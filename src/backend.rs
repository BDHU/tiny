use std::collections::{HashMap, VecDeque};
use tiny::{Agent, Decision, Event, Message, ToolCall};
use tokio::sync::{mpsc, oneshot};

pub(crate) type PermissionId = u64;

pub(crate) enum BackendCommand {
    Submit(String),
    PermissionDecision {
        id: PermissionId,
        decision: Decision,
    },
}

pub(crate) enum BackendEvent {
    Message(Message),
    PermissionRequest { id: PermissionId, call: ToolCall },
    TurnStarted,
    TurnError(String),
    TurnDone,
}

pub(crate) struct Backend {
    pub(crate) commands: mpsc::UnboundedSender<BackendCommand>,
    pub(crate) events: mpsc::UnboundedReceiver<BackendEvent>,
}

pub(crate) fn spawn(agent: Agent) -> Backend {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel();
    tokio::spawn(run(agent, cmd_rx, evt_tx));
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

async fn run(
    mut agent: Agent,
    mut commands: mpsc::UnboundedReceiver<BackendCommand>,
    events: mpsc::UnboundedSender<BackendEvent>,
) {
    let mut pending = Pending::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    while let Some(input) = next_input(&mut commands, &mut queue, &mut pending).await {
        run_turn(
            &mut agent,
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
) -> Option<String> {
    if let Some(input) = queue.pop_front() {
        return Some(input);
    }
    loop {
        match commands.recv().await? {
            BackendCommand::Submit(input) => return Some(input),
            BackendCommand::PermissionDecision { id, decision } => pending.resolve(id, decision),
        }
    }
}

async fn run_turn(
    agent: &mut Agent,
    input: String,
    commands: &mut mpsc::UnboundedReceiver<BackendCommand>,
    queue: &mut VecDeque<String>,
    pending: &mut Pending,
    events: &mpsc::UnboundedSender<BackendEvent>,
) {
    let _ = events.send(BackendEvent::TurnStarted);

    let (agent_tx, mut agent_rx) = mpsc::unbounded_channel();
    let turn = agent.send(input, &agent_tx);
    tokio::pin!(turn);

    loop {
        tokio::select! {
            _ = &mut turn => {
                while let Ok(event) = agent_rx.try_recv() {
                    forward(event, events, pending);
                }
                return;
            }
            Some(event) = agent_rx.recv() => forward(event, events, pending),
            Some(cmd) = commands.recv() => match cmd {
                BackendCommand::Submit(input) => queue.push_back(input),
                BackendCommand::PermissionDecision { id, decision } => pending.resolve(id, decision),
            },
        }
    }
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
