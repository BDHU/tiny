use crossterm::event::{self, Event, MouseEventKind};
use tokio::sync::mpsc;

pub(crate) enum ReaderEvent {
    Terminal(Event),
    Error(String),
}

pub(crate) fn spawn(tx: mpsc::UnboundedSender<ReaderEvent>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        match event::read() {
            Ok(event) => {
                if should_forward(&event) && tx.send(ReaderEvent::Terminal(event)).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = tx.send(ReaderEvent::Error(error.to_string()));
                break;
            }
        }
    })
}

fn should_forward(event: &Event) -> bool {
    match event {
        Event::Key(_) | Event::Paste(_) | Event::Resize(_, _) => true,
        Event::Mouse(mouse) => {
            matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            )
        }
        _ => false,
    }
}
