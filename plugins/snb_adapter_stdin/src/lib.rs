use std::io::{self, BufRead};
use std::sync::Arc;

use snb_core::adapter::{Adapter, run_async};
use snb_core::context::BotContext;
use snb_core::event::{ContentItem, Event, FileSource};
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
                "" => Event::message("stdin", text),
                "file" => parse_file_message(args).unwrap_or_else(|| Event::message("stdin", text)),
                _ => Event::command("stdin", cmd, args),
            }
        } else {
            Event::message("stdin", text)
        };
        bot.emit_event(event.with_sender("stdin"));
    }
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
        for item in &message.content {
            match item {
                ContentItem::Text(text) => println!("{text}"),
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
