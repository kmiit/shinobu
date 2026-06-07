use std::time::Duration;

/// Discriminant for [`Event`]. The structured payload (parsed command,
/// message text, etc.) lives in the corresponding `Option` field on the
/// event itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventType {
    PluginLoaded,
    PluginUnloaded,
    Command,
    Message,
    MessageSent,
    MessageDelete,
    Other(String),
}

/// A parsed command invocation.
///
/// `cmd` is the bare command name (without any leading prefix like `/`),
/// `args` is everything that followed it on the same line.
#[derive(Debug, Clone)]
pub struct Command {
    pub cmd: String,
    pub args: String,
}

/// Source of an image attachment.
#[derive(Debug, Clone)]
pub enum ImageSource {
    /// Remote URL.
    Url(String),
    /// Local image path.
    Path(String),
    /// Platform-assigned file ID.
    Id(String),
    /// Base64-encoded image data.
    Base64(String),
}

/// Source of a file attachment.
#[derive(Debug, Clone)]
pub enum FileSource {
    /// Remote URL.
    Url(String),
    /// Local file path.
    Path(String),
    /// Platform-assigned file ID.
    Id(String),
}

/// Text formatting mode for platforms that support parsed markup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFormat {
    Markdown,
    MarkdownV2,
    Html,
}

/// A single content item within a message.
///
/// A message can contain multiple content items (text + image + file, etc.).
#[derive(Debug, Clone)]
pub enum ContentItem {
    Text {
        text: String,
        format: Option<TextFormat>,
    },
    Image {
        source: ImageSource,
        /// Optional platform-assigned file ID.
        file_id: Option<String>,
        /// Optional image caption.
        caption: Option<String>,
    },
    File {
        source: FileSource,
        file_name: Option<String>,
        file_id: Option<String>,
    },
    /// Platform-specific custom content type.
    Other { kind: String, data: String },
}

impl ContentItem {
    /// Build a plain text content item.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            format: None,
        }
    }

    /// Build a formatted text content item.
    pub fn formatted_text(text: impl Into<String>, format: TextFormat) -> Self {
        Self::Text {
            text: text.into(),
            format: Some(format),
        }
    }
}

/// Chat context type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatType {
    /// Private / one-on-one chat.
    Private,
    /// Group chat.
    Group,
    /// Channel or guild (e.g., Discord guild, Telegram channel).
    Guild,
    /// Platform-specific custom type.
    Other(String),
}

/// A non-command message.
#[derive(Debug, Clone)]
pub struct Message {
    /// Message ID assigned by the adapter, used for reply references.
    pub id: Option<String>,
    /// ID of the message being replied to.
    pub reply_to: Option<String>,
    /// Content items (text, images, files, etc.).
    pub content: Vec<ContentItem>,
    /// Sender identifier (user ID, username, etc.).
    pub from: Option<String>,
    /// Recipient identifier (channel, group, user, etc.).
    pub to: Option<String>,
    /// Users mentioned / @-ed in this message.
    pub at: Vec<String>,
    /// Chat context type.
    pub chat_type: Option<ChatType>,
    /// Delete this outgoing message after the given duration, if supported by the adapter.
    pub delete_after: Option<Duration>,
}

impl Message {
    /// Concatenate all text content items into a single string.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                ContentItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Returns `true` if this message contains any text content.
    pub fn has_text(&self) -> bool {
        self.content
            .iter()
            .any(|c| matches!(c, ContentItem::Text { .. }))
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub event_type: EventType,
    pub source: String,
    /// Free-form metadata. Used by `PluginLoaded`/`PluginUnloaded` to carry
    /// the plugin name; otherwise typically empty.
    pub data: String,
    /// `Some` iff `event_type == EventType::Command`.
    pub command: Option<Command>,
    /// `Some` iff this event carries message-shaped data.
    pub message: Option<Message>,
    /// Sender plugin name. `Some` indicates the sender expects a response.
    pub sender: Option<String>,
    /// Target receiver plugin name. `Some` routes only to that plugin;
    /// `None` broadcasts to all plugins.
    pub receiver: Option<String>,
}

impl Event {
    /// Build an [`EventType::Command`] event.
    pub fn command(
        source: impl Into<String>,
        cmd: impl Into<String>,
        args: impl Into<String>,
    ) -> Self {
        Self {
            event_type: EventType::Command,
            source: source.into(),
            data: String::new(),
            command: Some(Command {
                cmd: cmd.into(),
                args: args.into(),
            }),
            message: None,
            sender: None,
            receiver: None,
        }
    }

    /// Build an [`EventType::Message`] event with a text content item.
    pub fn message(source: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            event_type: EventType::Message,
            source: source.into(),
            data: String::new(),
            command: None,
            message: Some(Message {
                id: None,
                reply_to: None,
                content: vec![ContentItem::text(text)],
                from: None,
                to: None,
                at: Vec::new(),
                chat_type: None,
                delete_after: None,
            }),
            sender: None,
            receiver: None,
        }
    }

    /// Build an [`EventType::Message`] event with formatted text content.
    pub fn formatted_message(
        source: impl Into<String>,
        text: impl Into<String>,
        format: TextFormat,
    ) -> Self {
        Self {
            event_type: EventType::Message,
            source: source.into(),
            data: String::new(),
            command: None,
            message: Some(Message {
                id: None,
                reply_to: None,
                content: vec![ContentItem::formatted_text(text, format)],
                from: None,
                to: None,
                at: Vec::new(),
                chat_type: None,
                delete_after: None,
            }),
            sender: None,
            receiver: None,
        }
    }

    /// Build an [`EventType::Message`] event with a file content item.
    pub fn file_message(
        source: impl Into<String>,
        file: FileSource,
        file_name: Option<String>,
    ) -> Self {
        Self {
            event_type: EventType::Message,
            source: source.into(),
            data: String::new(),
            command: None,
            message: Some(Message {
                id: None,
                reply_to: None,
                content: vec![ContentItem::File {
                    source: file,
                    file_name,
                    file_id: None,
                }],
                from: None,
                to: None,
                at: Vec::new(),
                chat_type: None,
                delete_after: None,
            }),
            sender: None,
            receiver: None,
        }
    }

    /// Build an [`EventType::MessageSent`] event.
    ///
    /// `platform_message_id` is the native adapter message id. `request_id`, when
    /// present, is the outgoing framework message id supplied by the sender.
    pub fn message_sent(
        source: impl Into<String>,
        platform_message_id: impl Into<String>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            event_type: EventType::MessageSent,
            source: source.into(),
            data: String::new(),
            command: None,
            message: Some(Message {
                id: Some(platform_message_id.into()),
                reply_to: request_id,
                content: Vec::new(),
                from: None,
                to: None,
                at: Vec::new(),
                chat_type: None,
                delete_after: None,
            }),
            sender: None,
            receiver: None,
        }
    }

    /// Build an [`EventType::MessageDelete`] event.
    ///
    /// The target message id is carried in [`Message::id`]. Adapters should treat it
    /// as their native platform message id.
    pub fn message_delete(source: impl Into<String>, message_id: impl Into<String>) -> Self {
        Self {
            event_type: EventType::MessageDelete,
            source: source.into(),
            data: String::new(),
            command: None,
            message: Some(Message {
                id: Some(message_id.into()),
                reply_to: None,
                content: Vec::new(),
                from: None,
                to: None,
                at: Vec::new(),
                chat_type: None,
                delete_after: None,
            }),
            sender: None,
            receiver: None,
        }
    }

    /// Build a typed event with no structured payload (plugin lifecycle,
    /// `Other`, etc.). `data` carries free-form context such as a plugin
    /// name.
    pub fn typed(
        event_type: EventType,
        source: impl Into<String>,
        data: impl Into<String>,
    ) -> Self {
        Self {
            event_type,
            source: source.into(),
            data: data.into(),
            command: None,
            message: None,
            sender: None,
            receiver: None,
        }
    }

    /// Set the sender plugin name (signals that a response is expected).
    pub fn with_sender(mut self, sender: impl Into<String>) -> Self {
        self.sender = Some(sender.into());
        self
    }

    /// Set the target receiver plugin name (directed routing, no broadcast).
    pub fn with_receiver(mut self, receiver: impl Into<String>) -> Self {
        self.receiver = Some(receiver.into());
        self
    }
}
