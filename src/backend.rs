use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tiny::{
    session, Agent, AgentConfig, Decision, Event, Message, Session, SessionId, SessionMeta,
    ToolCall,
};
use tokio::sync::{mpsc, oneshot};

pub(crate) type PermissionId = u64;

pub(crate) enum BackendCommand {
    Submit(String),
    PermissionDecision {
        id: PermissionId,
        decision: Decision,
    },
    NewSession,
    ListSessions,
    SwitchSession(SessionId),
}

pub(crate) enum BackendEvent {
    Message(Message),
    PermissionRequest {
        id: PermissionId,
        call: ToolCall,
    },
    TurnStarted,
    TurnError(String),
    TurnDone,
    SessionChanged {
        meta: SessionMeta,
        history: Vec<Message>,
    },
    SessionsListed(Result<Vec<SessionMeta>, String>),
    SessionError(String),
}

pub(crate) struct Backend {
    pub(crate) commands: mpsc::UnboundedSender<BackendCommand>,
    pub(crate) events: mpsc::UnboundedReceiver<BackendEvent>,
}

pub(crate) fn spawn(config: Arc<AgentConfig>, model: String) -> Backend {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel();
    tokio::spawn(run(config, model, cmd_rx, evt_tx));
    Backend {
        commands: cmd_tx,
        events: evt_rx,
    }
}

struct Pending {
    map: HashMap<PermissionId, oneshot::Sender<Decision>>,
    next_id: PermissionId,
}

impl Pending {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            next_id: 0,
        }
    }

    fn register(&mut self, reply: oneshot::Sender<Decision>) -> PermissionId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.map.insert(id, reply);
        id
    }

    fn resolve(&mut self, id: PermissionId, decision: Decision) {
        if let Some(reply) = self.map.remove(&id) {
            let _ = reply.send(decision);
        }
    }
}

struct Active {
    session: Session,
    agent: Agent,
}

impl Active {
    fn fresh(config: &Arc<AgentConfig>, model: &str) -> Self {
        let session = Session::new(model);
        let agent = Agent::new(config.clone(), Vec::new());
        Self { session, agent }
    }

    fn from_session(config: &Arc<AgentConfig>, session: Session) -> Self {
        let agent = Agent::new(config.clone(), session.history.clone());
        Self { session, agent }
    }

    fn meta(&self) -> SessionMeta {
        SessionMeta {
            id: self.session.id.clone(),
            updated_at: self.session.updated_at.clone(),
            title: self.session.title.clone(),
            model: self.session.model.clone(),
        }
    }
}

async fn run(
    config: Arc<AgentConfig>,
    model: String,
    mut commands: mpsc::UnboundedReceiver<BackendCommand>,
    events: mpsc::UnboundedSender<BackendEvent>,
) {
    let mut pending = Pending::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut active = Active::fresh(&config, &model);
    announce_session(&active, &events);

    loop {
        let Some(input) = next_input(
            &mut commands,
            &mut queue,
            &mut pending,
            &mut active,
            &config,
            &events,
        )
        .await
        else {
            break;
        };
        run_turn(
            &mut active,
            input,
            &mut commands,
            &mut queue,
            &mut pending,
            &events,
        )
        .await;
    }
}

async fn next_input(
    commands: &mut mpsc::UnboundedReceiver<BackendCommand>,
    queue: &mut VecDeque<String>,
    pending: &mut Pending,
    active: &mut Active,
    config: &Arc<AgentConfig>,
    events: &mpsc::UnboundedSender<BackendEvent>,
) -> Option<String> {
    if let Some(input) = queue.pop_front() {
        return Some(input);
    }
    loop {
        match commands.recv().await? {
            BackendCommand::Submit(input) => return Some(input),
            BackendCommand::PermissionDecision { id, decision } => pending.resolve(id, decision),
            BackendCommand::NewSession => {
                handle_new_session(active, config, events);
            }
            BackendCommand::ListSessions => {
                handle_list_sessions(events);
            }
            BackendCommand::SwitchSession(id) => {
                handle_switch_session(active, config, &id, events);
            }
        }
    }
}

async fn run_turn(
    active: &mut Active,
    input: String,
    commands: &mut mpsc::UnboundedReceiver<BackendCommand>,
    queue: &mut VecDeque<String>,
    pending: &mut Pending,
    events: &mpsc::UnboundedSender<BackendEvent>,
) {
    let _ = events.send(BackendEvent::TurnStarted);

    {
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel();
        let turn = active.agent.send(input, &agent_tx);
        tokio::pin!(turn);

        loop {
            tokio::select! {
                _ = &mut turn => {
                    while let Ok(event) = agent_rx.try_recv() {
                        forward(event, events, pending);
                    }
                    break;
                }
                Some(event) = agent_rx.recv() => forward(event, events, pending),
                Some(cmd) = commands.recv() => match cmd {
                    BackendCommand::Submit(input) => queue.push_back(input),
                    BackendCommand::PermissionDecision { id, decision } => pending.resolve(id, decision),
                    // Session changes mid-turn are rejected at the UI; drop any that slip through.
                    BackendCommand::NewSession | BackendCommand::ListSessions | BackendCommand::SwitchSession(_) => {}
                },
            }
        }
    }

    persist_active(active, events);
}

fn handle_new_session(
    active: &mut Active,
    config: &Arc<AgentConfig>,
    events: &mpsc::UnboundedSender<BackendEvent>,
) {
    let model = active.session.model.clone();
    *active = Active::fresh(config, &model);
    announce_session(active, events);
}

fn handle_list_sessions(events: &mpsc::UnboundedSender<BackendEvent>) {
    let result = session::list().map_err(|error| error.to_string());
    let _ = events.send(BackendEvent::SessionsListed(result));
}

fn handle_switch_session(
    active: &mut Active,
    config: &Arc<AgentConfig>,
    id: &SessionId,
    events: &mpsc::UnboundedSender<BackendEvent>,
) {
    match session::load(id) {
        Ok(session) => {
            *active = Active::from_session(config, session);
            announce_session(active, events);
        }
        Err(error) => {
            let _ = events.send(BackendEvent::SessionError(format!(
                "load {}: {error}",
                id.as_str()
            )));
        }
    }
}

fn persist_active(active: &mut Active, events: &mpsc::UnboundedSender<BackendEvent>) {
    if active.agent.history.len() <= active.session.history.len() {
        return;
    }
    active.session.history = active.agent.history.clone();
    active.session.ensure_title();
    active.session.touch();

    if let Err(error) = session::save(&active.session) {
        let _ = events.send(BackendEvent::SessionError(format!(
            "save {}: {error}",
            active.session.id.as_str()
        )));
    }
}

fn announce_session(active: &Active, events: &mpsc::UnboundedSender<BackendEvent>) {
    let _ = events.send(BackendEvent::SessionChanged {
        meta: active.meta(),
        history: active.agent.history.clone(),
    });
}

fn forward(event: Event, events: &mpsc::UnboundedSender<BackendEvent>, pending: &mut Pending) {
    let translated = match event {
        Event::Message(m) => BackendEvent::Message(m),
        Event::PermissionRequest { call, reply } => {
            let id = pending.register(reply);
            BackendEvent::PermissionRequest { id, call }
        }
        Event::TurnError(e) => BackendEvent::TurnError(e),
        Event::TurnDone => BackendEvent::TurnDone,
    };
    let _ = events.send(translated);
}
