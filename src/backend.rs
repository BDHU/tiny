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
    Shutdown,
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
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(run(agent, command_rx, event_tx));

    Backend {
        commands: command_tx,
        events: event_rx,
    }
}

async fn run(
    mut agent: Agent,
    mut commands: mpsc::UnboundedReceiver<BackendCommand>,
    events: mpsc::UnboundedSender<BackendEvent>,
) {
    let mut queue = VecDeque::new();
    let mut next_permission_id = 0;

    loop {
        if let Some(input) = queue.pop_front() {
            if !run_turn(
                &mut agent,
                input,
                &mut commands,
                &mut queue,
                &events,
                &mut next_permission_id,
            )
            .await
            {
                break;
            }
            continue;
        }

        match commands.recv().await {
            Some(BackendCommand::Submit(input)) => queue.push_back(input),
            Some(BackendCommand::Shutdown) | None => break,
            Some(BackendCommand::PermissionDecision { .. }) => {}
        }
    }
}

async fn run_turn(
    agent: &mut Agent,
    input: String,
    commands: &mut mpsc::UnboundedReceiver<BackendCommand>,
    queue: &mut VecDeque<String>,
    events: &mpsc::UnboundedSender<BackendEvent>,
    next_permission_id: &mut PermissionId,
) -> bool {
    let _ = events.send(BackendEvent::TurnStarted);

    let (agent_event_tx, mut agent_events) = mpsc::unbounded_channel();
    let turn = agent.send(input, &agent_event_tx);
    tokio::pin!(turn);

    let mut pending_permissions = HashMap::new();

    loop {
        tokio::select! {
            result = &mut turn => {
                let _ = result;
                while let Ok(event) = agent_events.try_recv() {
                    forward_agent_event(event, events, &mut pending_permissions, next_permission_id);
                }
                return true;
            }

            Some(event) = agent_events.recv() => {
                forward_agent_event(event, events, &mut pending_permissions, next_permission_id);
            }

            Some(command) = commands.recv() => match command {
                BackendCommand::Submit(input) => queue.push_back(input),
                BackendCommand::PermissionDecision { id, decision } => {
                    if let Some(reply) = pending_permissions.remove(&id) {
                        let _ = reply.send(decision);
                    }
                }
                BackendCommand::Shutdown => return false,
            },

            else => return false,
        }
    }
}

fn forward_agent_event(
    event: Event,
    events: &mpsc::UnboundedSender<BackendEvent>,
    pending_permissions: &mut HashMap<PermissionId, oneshot::Sender<Decision>>,
    next_permission_id: &mut PermissionId,
) {
    match event {
        Event::Message(message) => {
            let _ = events.send(BackendEvent::Message(message));
        }
        Event::PermissionRequest { call, reply } => {
            let id = *next_permission_id;
            *next_permission_id = (*next_permission_id).wrapping_add(1);
            pending_permissions.insert(id, reply);
            let _ = events.send(BackendEvent::PermissionRequest { id, call });
        }
        Event::TurnError(error) => {
            let _ = events.send(BackendEvent::TurnError(error));
        }
        Event::TurnDone => {
            let _ = events.send(BackendEvent::TurnDone);
        }
    }
}
