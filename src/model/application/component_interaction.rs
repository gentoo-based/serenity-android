use serde::de::Error as DeError;
use serde::ser::{Serialize, SerializeMap as _};

#[cfg(feature = "model")]
use crate::builder::{
    Builder,
    CreateInteractionResponse,
    CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage,
    EditInteractionResponse,
};
#[cfg(feature = "collector")]
use crate::client::Context;
#[cfg(feature = "model")]
use crate::http::{CacheHttp, Http};
use crate::internal::prelude::*;
use crate::json;
use crate::model::prelude::*;
#[cfg(all(feature = "collector", feature = "utils"))]
use crate::utils::{CreateQuickModal, QuickModalResponse};

/// An interaction triggered by a message component.
///
/// [Discord docs](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-object-interaction-structure).
#[cfg_attr(feature = "typesize", derive(typesize::derive::TypeSize))]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(remote = "Self")]
#[non_exhaustive]
pub struct ComponentInteraction {
    /// Id of the interaction.
    pub id: InteractionId,
    /// Id of the application this interaction is for.
    pub application_id: ApplicationId,
    /// The data of the interaction which was triggered.
    pub data: ComponentInteractionData,
    /// The guild Id this interaction was sent from, if there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<GuildId>,
    /// Channel that the interaction was sent from.
    pub channel: Option<PartialChannel>,
    /// The channel Id this interaction was sent from.
    pub channel_id: ChannelId,
    /// The `member` data for the invoking user.
    ///
    /// **Note**: It is only present if the interaction is triggered in a guild.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<Member>,
    /// The `user` object for the invoking user.
    #[serde(default)]
    pub user: User,
    /// A continuation token for responding to the interaction.
    pub token: String,
    /// Always `1`.
    pub version: u8,
    /// The message this interaction was triggered by, if it is a component.
    pub message: Box<Message>,
    /// Permissions the app or bot has within the channel the interaction was sent from.
    pub app_permissions: Option<Permissions>,
    /// The selected language of the invoking user.
    pub locale: String,
    /// The guild's preferred locale.
    pub guild_locale: Option<String>,
    /// For monetized applications, any entitlements of the invoking user.
    pub entitlements: Vec<Entitlement>,
    /// The owners of the applications that authorized the interaction, such as a guild or user.
    #[serde(default)]
    pub authorizing_integration_owners: AuthorizingIntegrationOwners,
    /// The context where the interaction was triggered from.
    pub context: Option<InteractionContext>,
}

#[cfg(feature = "model")]
impl ComponentInteraction {
    /// Gets the interaction response.
    ///
    /// # Errors
    ///
    /// Returns an [`Error::Http`] if there is no interaction response.
    pub async fn get_response(&self, http: impl AsRef<Http>) -> Result<Message> {
        http.as_ref().get_original_interaction_response(&self.token).await
    }

    /// Creates a response to the interaction received.
    ///
    /// **Note**: Message contents must be under 2000 unicode code points.
    ///
    /// # Errors
    ///
    /// Returns an [`Error::Model`] if the message content is too long. May also return an
    /// [`Error::Http`] if the API returns an error, or an [`Error::Json`] if there is an error in
    /// deserializing the API response.
    pub async fn create_response(
        &self,
        cache_http: impl CacheHttp,
        builder: CreateInteractionResponse,
    ) -> Result<()> {
        builder.execute(cache_http, (self.id, &self.token)).await
    }

    /// Edits the initial interaction response.
    ///
    /// **Note**: Message contents must be under 2000 unicode code points.
    ///
    /// # Errors
    ///
    /// Returns an [`Error::Model`] if the message content is too long. May also return an
    /// [`Error::Http`] if the API returns an error, or an [`Error::Json`] if there is an error in
    /// deserializing the API response.
    pub async fn edit_response(
        &self,
        cache_http: impl CacheHttp,
        builder: EditInteractionResponse,
    ) -> Result<Message> {
        builder.execute(cache_http, &self.token).await
    }

    /// Deletes the initial interaction response.
    ///
    /// Does not work on ephemeral messages.
    ///
    /// # Errors
    ///
    /// May return [`Error::Http`] if the API returns an error. Such as if the response was already
    /// deleted.
    pub async fn delete_response(&self, http: impl AsRef<Http>) -> Result<()> {
        http.as_ref().delete_original_interaction_response(&self.token).await
    }

    /// Creates a followup response to the response sent.
    ///
    /// **Note**: Message contents must be under 2000 unicode code points.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Model`] if the content is too long. May also return [`Error::Http`] if the
    /// API returns an error, or [`Error::Json`] if there is an error in deserializing the
    /// response.
    pub async fn create_followup(
        &self,
        cache_http: impl CacheHttp,
        builder: CreateInteractionResponseFollowup,
    ) -> Result<Message> {
        builder.execute(cache_http, (None, &self.token)).await
    }

    /// Edits a followup response to the response sent.
    ///
    /// **Note**: Message contents must be under 2000 unicode code points.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Model`] if the content is too long. May also return [`Error::Http`] if the
    /// API returns an error, or [`Error::Json`] if there is an error in deserializing the
    /// response.
    pub async fn edit_followup(
        &self,
        cache_http: impl CacheHttp,
        message_id: impl Into<MessageId>,
        builder: CreateInteractionResponseFollowup,
    ) -> Result<Message> {
        builder.execute(cache_http, (Some(message_id.into()), &self.token)).await
    }

    /// Deletes a followup message.
    ///
    /// # Errors
    ///
    /// May return [`Error::Http`] if the API returns an error. Such as if the response was already
    /// deleted.
    pub async fn delete_followup<M: Into<MessageId>>(
        &self,
        http: impl AsRef<Http>,
        message_id: M,
    ) -> Result<()> {
        http.as_ref().delete_followup_message(&self.token, message_id.into()).await
    }

    /// Gets a followup message.
    ///
    /// # Errors
    ///
    /// May return [`Error::Http`] if the API returns an error. Such as if the response was
    /// deleted.
    pub async fn get_followup<M: Into<MessageId>>(
        &self,
        http: impl AsRef<Http>,
        message_id: M,
    ) -> Result<Message> {
        http.as_ref().get_followup_message(&self.token, message_id.into()).await
    }

    /// Helper function to defer an interaction.
    ///
    /// # Errors
    ///
    /// Returns an [`Error::Http`] if the API returns an error, or an [`Error::Json`] if there is
    /// an error in deserializing the API response.
    pub async fn defer(&self, cache_http: impl CacheHttp) -> Result<()> {
        self.create_response(cache_http, CreateInteractionResponse::Acknowledge).await
    }

    /// Helper function to defer an interaction ephemerally
    ///
    /// # Errors
    ///
    /// May also return an [`Error::Http`] if the API returns an error, or an [`Error::Json`] if
    /// there is an error in deserializing the API response.
    pub async fn defer_ephemeral(&self, cache_http: impl CacheHttp) -> Result<()> {
        let builder = CreateInteractionResponse::Defer(
            CreateInteractionResponseMessage::new().ephemeral(true),
        );
        self.create_response(cache_http, builder).await
    }

    /// See [`CreateQuickModal`].
    ///
    /// # Errors
    ///
    /// See [`CreateQuickModal::execute()`].
    #[cfg(all(feature = "collector", feature = "utils"))]
    pub async fn quick_modal(
        &self,
        ctx: &Context,
        builder: CreateQuickModal,
    ) -> Result<Option<QuickModalResponse>> {
        builder.execute(ctx, self.id, &self.token).await
    }
}

// Manual impl needed to insert guild_id into model data
impl<'de> Deserialize<'de> for ComponentInteraction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> StdResult<Self, D::Error> {
        // calls #[serde(remote)]-generated inherent method
        let mut interaction = Self::deserialize(deserializer)?;
        if let (Some(guild_id), Some(member)) = (interaction.guild_id, &mut interaction.member) {
            member.guild_id = guild_id;
            // If `member` is present, `user` wasn't sent and is still filled with default data
            interaction.user = member.user.clone();
        }
        Ok(interaction)
    }
}

impl Serialize for ComponentInteraction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> StdResult<S::Ok, S::Error> {
        // calls #[serde(remote)]-generated inherent method
        Self::serialize(self, serializer)
    }
}

#[cfg_attr(feature = "typesize", derive(typesize::derive::TypeSize))]
#[derive(Clone, Debug)]
pub enum ComponentInteractionDataKind {
    Button,
    StringSelect { values: Vec<String> },
    UserSelect { values: Vec<UserId> },
    RoleSelect { values: Vec<RoleId> },
    MentionableSelect { values: Vec<GenericId> },
    ChannelSelect { values: Vec<ChannelId> },
    Unknown(u8),
}

// Manual impl needed to emulate integer enum tags
impl<'de> Deserialize<'de> for ComponentInteractionDataKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> StdResult<Self, D::Error> {
        #[derive(Deserialize)]
        struct Json {
            component_type: ComponentType,
            values: Option<json::Value>,
        }
        let json = Json::deserialize(deserializer)?;

        macro_rules! parse_values {
            () => {
                json::from_value(json.values.ok_or_else(|| D::Error::missing_field("values"))?)
                    .map_err(D::Error::custom)?
            };
        }

        Ok(match json.component_type {
            ComponentType::Button => Self::Button,
            ComponentType::StringSelect => Self::StringSelect {
                values: parse_values!(),
            },
            ComponentType::UserSelect => Self::UserSelect {
                values: parse_values!(),
            },
            ComponentType::RoleSelect => Self::RoleSelect {
                values: parse_values!(),
            },
            ComponentType::MentionableSelect => Self::MentionableSelect {
                values: parse_values!(),
            },
            ComponentType::ChannelSelect => Self::ChannelSelect {
                values: parse_values!(),
            },
            ComponentType::Unknown(x) => Self::Unknown(x),
            x @ (ComponentType::ActionRow | ComponentType::InputText) => {
                return Err(D::Error::custom(format_args!(
                    "invalid message component type in this context: {x:?}",
                )));
            },
        })
    }
}

impl Serialize for ComponentInteractionDataKind {
    #[rustfmt::skip] // Remove this for horror.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> StdResult<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("component_type", &match self {
            Self::Button { .. } => 2,
            Self::StringSelect { .. } => 3,
            Self::UserSelect { .. } => 5,
            Self::RoleSelect { .. } => 6,
            Self::MentionableSelect { .. } => 7,
            Self::ChannelSelect { .. } => 8,
            Self::Unknown(x) => *x,
        })?;

        match self {
            Self::StringSelect { values } => map.serialize_entry("values", values)?,
            Self::UserSelect { values } => map.serialize_entry("values", values)?,
            Self::RoleSelect { values } => map.serialize_entry("values", values)?,
            Self::MentionableSelect { values } => map.serialize_entry("values", values)?,
            Self::ChannelSelect { values } => map.serialize_entry("values", values)?,
            Self::Button | Self::Unknown(_) => map.serialize_entry("values", &None::<()>)?,
        }

        map.end()
    }
}

/// A message component interaction data, provided by [`ComponentInteraction::data`]
///
/// [Discord docs](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-object-message-component-data-structure).
#[cfg_attr(feature = "typesize", derive(typesize::derive::TypeSize))]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ComponentInteractionData {
    /// The custom id of the component.
    pub custom_id: String,
    /// Type and type-specific data of this component interaction.
    #[serde(flatten)]
    pub kind: ComponentInteractionDataKind,
}
