use crossterm::event::{self, Event};
use tokio::sync::mpsc;

pub(crate) fn spawn(tx: mpsc::UnboundedSender<Event>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        match event::read() {
            Ok(event) if forward(&event) => {
                if tx.send(event).is_err() {
                    break;
                }
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    })
}

fn forward(event: &Event) -> bool {
    matches!(event, Event::Key(_) | Event::Paste(_) | Event::Resize(_, _))
}
