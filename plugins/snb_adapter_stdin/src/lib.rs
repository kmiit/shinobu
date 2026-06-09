use std::io::{self, BufRead};
use std::sync::Arc;

use snb_core::adapter::{Adapter, run_async};
use snb_core::context::BotContext;
use snb_core::event::{ChatType, ContentItem, Event, EventType, FileSource, Message};
use snb_macros::plugin;

/// Built-in stdin adapter.
///
/// Reads lines from stdin and dispatches them as [`snb_core::event::EventType::Message`]
/// events through [`BotContext::emit_event`].
///
/// This also serves as a reference implementation for third-party adapters: the
/// `#[plugin(...)]` form generates the whole `SnbPlugin` impl, and `#[adapter]` /
/// `#[command]` / `#[hook]` / `#[message_handler]` declare and auto-register the
/// plugin's components.
#[plugin(name = "stdin", version = "0.1.0", kind = Adapter)]
pub struct StdinAdapter;

async fn stdin_reader(bot: Arc<dyn BotContext>) {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let text = match line {
            Ok(t) if !t.is_empty() => t,
            _ => break,
        };
        let event = if let Some(rest) = text.strip_prefix('/') {
            let mut parts = rest.splitn(2, char::is_whitespace);
            let cmd = parts.next().unwrap_or("");
            let args = parts.next().unwrap_or("").trim_start();
            match cmd {
                "" => Event::message("stdin", text.as_str()),
                "file" => parse_file_message(args)
                    .unwrap_or_else(|| Event::message("stdin", text.as_str())),
                _ => Event::command("stdin", cmd, args),
            }
        } else {
            Event::message("stdin", text.as_str())
        };
        bot.emit_event(with_admin_context(event, &text).with_sender("stdin"));
    }
}

fn with_admin_context(mut event: Event, text: &str) -> Event {
    if let Some(message) = event.message.as_mut() {
        message.from.get_or_insert_with(|| "stdin".to_string());
        message.to.get_or_insert_with(|| "stdin".to_string());
        message.chat_type.get_or_insert(ChatType::Private);
        message.is_admin = true;
    } else {
        event.message = Some(Message {
            id: None,
            reply_to: None,
            content: vec![ContentItem::text(text)],
            from: Some("stdin".to_string()),
            to: Some("stdin".to_string()),
            at: Vec::new(),
            chat_type: Some(ChatType::Private),
            is_admin: true,
            delete_after: None,
        });
    }
    event
}

fn parse_file_message(args: &str) -> Option<Event> {
    let mut parts = args.splitn(2, char::is_whitespace);
    let path = parts.next()?.trim();
    if path.is_empty() {
        return None;
    }
    let file_name = parts.next().map(str::trim).filter(|s| !s.is_empty());
    Some(Event::file_message(
        "stdin",
        FileSource::Path(path.to_string()),
        file_name.map(str::to_string),
    ))
}

impl Adapter for StdinAdapter {
    fn run(&self, bot: Arc<dyn BotContext>) {
        run_async(stdin_reader(bot));
    }

    fn send(&self, event: &Event) -> anyhow::Result<()> {
        let Some(message) = &event.message else {
            return Ok(());
        };
        if event.event_type == EventType::MessageDelete {
            if let Some(id) = &message.id {
                println!("[delete] message id={id}");
            }
            return Ok(());
        }
        let prefix = if event.event_type == EventType::MessageEdit {
            "[edit] "
        } else {
            ""
        };
        for item in &message.content {
            match item {
                ContentItem::Text { text, .. } => println!("{prefix}{text}"),
                ContentItem::File {
                    source,
                    file_name,
                    file_id,
                } => {
                    let source = match source {
                        FileSource::Url(url) => format!("url={url}"),
                        FileSource::Path(path) => format!("path={path}"),
                        FileSource::Id(id) => format!("id={id}"),
                    };
                    println!(
                        "[file] {source} name={} id={}",
                        file_name.as_deref().unwrap_or("-"),
                        file_id.as_deref().unwrap_or("-")
                    );
                }
                ContentItem::Image {
                    source,
                    file_id,
                    caption,
                } => {
                    println!(
                        "[image] source={source:?} id={} caption={}",
                        file_id.as_deref().unwrap_or("-"),
                        caption.as_deref().unwrap_or("-")
                    );
                }
                ContentItem::Other { kind, data } => println!("[{kind}] {data}"),
            }
        }
        Ok(())
    }
}

snb_core::registry::submit! {
    snb_core::registry::AdapterRegistration {
        factory: || Arc::new(StdinAdapter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_message_defaults_to_admin() {
        let event = with_admin_context(Event::message("stdin", "hello"), "hello");
        let message = event.message.unwrap();

        assert_eq!(message.from.as_deref(), Some("stdin"));
        assert_eq!(message.to.as_deref(), Some("stdin"));
        assert_eq!(message.chat_type, Some(ChatType::Private));
        assert!(message.is_admin);
    }

    #[test]
    fn stdin_command_carries_admin_message() {
        let event = with_admin_context(Event::command("stdin", "ping", ""), "/ping");
        let message = event.message.unwrap();

        assert_eq!(message.text(), "/ping");
        assert!(message.is_admin);
    }
}
