use std::path::Path;

use tokio::fs::File;
use tokio::io::AsyncReadExt;
#[cfg(feature = "http")]
use url::Url;

use crate::all::Message;
#[cfg(feature = "http")]
use crate::error::Error;
use crate::error::Result;
#[cfg(feature = "http")]
use crate::http::Http;
use crate::model::id::AttachmentId;

/// A builder for creating a new attachment from a file path, file data, or URL.
///
/// [Discord docs](https://discord.com/developers/docs/resources/channel#attachment-object-attachment-structure).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
#[must_use]
pub struct CreateAttachment {
    pub(crate) id: u64, // Placeholder ID will be filled in when sending the request
    pub filename: String,
    pub description: Option<String>,

    #[serde(skip)]
    pub data: Vec<u8>,
}

impl CreateAttachment {
    /// Builds an [`CreateAttachment`] from the raw attachment data.
    pub fn bytes(data: impl Into<Vec<u8>>, filename: impl Into<String>) -> CreateAttachment {
        CreateAttachment {
            data: data.into(),
            filename: filename.into(),
            description: None,
            id: 0,
        }
    }

    /// Builds an [`CreateAttachment`] by reading a local file.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if reading the file fails.
    pub async fn path(path: impl AsRef<Path>) -> Result<CreateAttachment> {
        let mut file = File::open(path.as_ref()).await?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).await?;

        let filename = path
            .as_ref()
            .file_name()
            .ok_or_else(|| std::io::Error::other("attachment path must not be a directory"))?;

        Ok(CreateAttachment::bytes(data, filename.to_string_lossy().to_string()))
    }

    /// Builds an [`CreateAttachment`] by reading from a file handler.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] error if reading the file fails.
    pub async fn file(file: &File, filename: impl Into<String>) -> Result<CreateAttachment> {
        let mut data = Vec::new();
        file.try_clone().await?.read_to_end(&mut data).await?;

        Ok(CreateAttachment::bytes(data, filename))
    }

    /// Builds an [`CreateAttachment`] by downloading attachment data from a URL.
    ///
    /// # Errors
    ///
    /// [`Error::Url`] if the URL is invalid, [`Error::Http`] if downloading the data fails.
    #[cfg(feature = "http")]
    pub async fn url(http: impl AsRef<Http>, url: &str) -> Result<CreateAttachment> {
        let url = Url::parse(url).map_err(|_| Error::Url(url.to_string()))?;

        let response = http.as_ref().client.get(url.clone()).send().await?;
        let data = response.bytes().await?.to_vec();

        let filename = url
            .path_segments()
            .and_then(Iterator::last)
            .ok_or_else(|| Error::Url(url.to_string()))?;

        Ok(CreateAttachment::bytes(data, filename))
    }

    /// Converts the stored data to the base64 representation.
    ///
    /// This is used in the library internally because Discord expects image data as base64 in many
    /// places.
    #[must_use]
    pub fn to_base64(&self) -> String {
        use base64::engine::{Config, Engine};

        const PREFIX: &str = "data:image/png;base64,";

        let engine = base64::prelude::BASE64_STANDARD;
        let encoded_size = base64::encoded_len(self.data.len(), engine.config().encode_padding())
            .and_then(|len| len.checked_add(PREFIX.len()))
            .expect("buffer capacity overflow");

        let mut encoded = String::with_capacity(encoded_size);
        encoded.push_str(PREFIX);
        engine.encode_string(&self.data, &mut encoded);
        encoded
    }

    /// Sets a description for the file (max 1024 characters).
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
struct ExistingAttachment {
    id: AttachmentId,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(untagged)]
enum NewOrExisting {
    New(CreateAttachment),
    Existing(ExistingAttachment),
}

/// You can add new attachments and edit existing ones using this builder.
///
/// When this builder is _not_ supplied in a message edit, Discord keeps the attachments intact.
/// However, as soon as a builder is supplied, Discord removes all attachments from the message. If
/// you want to keep old attachments, you must specify this either using [`Self::keep_all`], or
/// individually for each attachment using [`Self::keep`].
///
/// # Examples
///
/// ## Removing all attachments
///
/// ```rust,no_run
/// # use serenity::all::*;
/// # async fn foo_(ctx: Http, mut msg: Message) -> Result<(), Error> {
/// msg.edit(ctx, EditMessage::new().attachments(EditAttachments::new())).await?;
/// # Ok(()) }
/// ```
///
/// ## Adding a new attachment without deleting existing attachments
///
/// ```rust,no_run
/// # use serenity::all::*;
/// # async fn foo_(ctx: Http, mut msg: Message, my_attachment: CreateAttachment) -> Result<(), Error> {
/// msg.edit(ctx, EditMessage::new().attachments(
///     EditAttachments::keep_all(&msg).add(my_attachment)
/// )).await?;
/// # Ok(()) }
/// ```
///
/// ## Delete all but the first attachment
///
/// ```rust,no_run
/// # use serenity::all::*;
/// # async fn foo_(ctx: Http, mut msg: Message, my_attachment: CreateAttachment) -> Result<(), Error> {
/// msg.edit(ctx, EditMessage::new().attachments(
///     EditAttachments::new().keep(msg.attachments[0].id)
/// )).await?;
/// # Ok(()) }
/// ```
///
/// ## Delete only the first attachment
///
/// ```rust,no_run
/// # use serenity::all::*;
/// # async fn foo_(ctx: Http, mut msg: Message, my_attachment: CreateAttachment) -> Result<(), Error> {
/// msg.edit(ctx, EditMessage::new().attachments(
///     EditAttachments::keep_all(&msg).remove(msg.attachments[0].id)
/// )).await?;
/// # Ok(()) }
/// ```
///
/// # Notes
///
/// Internally, this type is used not just for message editing endpoints, but also for message
/// creation endpoints.
#[derive(Default, Debug, Clone, serde::Serialize, PartialEq)]
#[serde(transparent)]
#[must_use]
pub struct EditAttachments {
    new_and_existing_attachments: Vec<NewOrExisting>,
}

impl EditAttachments {
    /// An empty attachments builder.
    ///
    /// Existing attachments are not kept by default, either. See [`Self::keep_all()`] or
    /// [`Self::keep()`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new attachments builder that keeps all existing attachments.
    ///
    /// Shorthand for [`Self::new()`] and calling [`Self::keep()`] for every [`AttachmentId`] in
    /// [`Message::attachments`].
    ///
    /// If you only want to keep a subset of attachments from the message, either implement this
    /// method manually, or use [`Self::remove()`].
    ///
    /// **Note: this EditAttachments must be run on the same message as is supplied here, or else
    /// Discord will throw an error!**
    pub fn keep_all(msg: &Message) -> Self {
        Self {
            new_and_existing_attachments: msg
                .attachments
                .iter()
                .map(|a| {
                    NewOrExisting::Existing(ExistingAttachment {
                        id: a.id,
                    })
                })
                .collect(),
        }
    }

    /// This method adds an existing attachment to the list of attachments that are kept after
    /// editing.
    ///
    /// Opposite of [`Self::remove`].
    pub fn keep(mut self, id: AttachmentId) -> Self {
        self.new_and_existing_attachments.push(NewOrExisting::Existing(ExistingAttachment {
            id,
        }));
        self
    }

    /// This method removes an existing attachment from the list of attachments that are kept after
    /// editing.
    ///
    /// Opposite of [`Self::keep`].
    pub fn remove(mut self, id: AttachmentId) -> Self {
        #[allow(clippy::match_like_matches_macro)] // `matches!` is less clear here
        self.new_and_existing_attachments.retain(|a| match a {
            NewOrExisting::Existing(a) if a.id == id => false,
            _ => true,
        });
        self
    }

    /// Adds a new attachment to the attachment list.
    #[allow(clippy::should_implement_trait)] // Clippy thinks add == std::ops::Add::add
    pub fn add(mut self, attachment: CreateAttachment) -> Self {
        self.new_and_existing_attachments.push(NewOrExisting::New(attachment));
        self
    }

    /// Clones all new attachments into a new Vec, keeping only data and filename, because those
    /// are needed for the multipart form data. The data is taken out of `self` in the process, so
    /// this method can only be called once.
    pub(crate) fn take_files(&mut self) -> Vec<CreateAttachment> {
        let mut id_placeholder = 0;

        let mut files = Vec::new();
        for attachment in &mut self.new_and_existing_attachments {
            if let NewOrExisting::New(attachment) = attachment {
                let mut cloned_attachment = CreateAttachment::bytes(
                    std::mem::take(&mut attachment.data),
                    attachment.filename.clone(),
                );

                // Assign placeholder IDs so Discord can match metadata to file contents
                attachment.id = id_placeholder;
                cloned_attachment.id = id_placeholder;
                files.push(cloned_attachment);

                id_placeholder += 1;
            }
        }
        files
    }

    #[cfg(feature = "cache")]
    pub(crate) fn is_empty(&self) -> bool {
        self.new_and_existing_attachments.is_empty()
    }
}
