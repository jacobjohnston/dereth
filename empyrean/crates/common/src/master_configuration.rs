// Ported from ACE (ACEmulator), AGPL-3.0: Source/ACE.Common/MasterConfiguration.cs
//! `MasterConfiguration`: the root of the configuration (`empyrean.toml`, read by
//! [`crate::toml_config`]). The serde names are ACE's `Config.js` names, which the one-time
//! `Config.js` converter reads.

use serde::{Deserialize, Serialize};

use crate::dat_overlay_configuration::DatOverlayConfiguration;
use crate::database_configuration::DatabaseConfiguration;
use crate::ddd_configuration::DDDConfiguration;
use crate::era::EraConfiguration;
use crate::federation_configuration::FederationConfiguration;
use crate::game_configuration::GameConfiguration;
use crate::offline_configuration::OfflineConfiguration;
use crate::status_configuration::StatusConfiguration;

// ACE: MasterConfiguration
/// The whole configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterConfiguration {
    // ACE: MasterConfiguration.Server
    /// `Server`.
    #[serde(rename = "Server")]
    pub server: GameConfiguration,

    // ACE: MasterConfiguration.MySql
    /// `MySql` in ACE; `[database]` in `empyrean.toml`.
    #[serde(rename = "MySql")]
    pub database: DatabaseConfiguration,

    // ACE: MasterConfiguration.Offline
    /// `Offline`.
    #[serde(rename = "Offline")]
    pub offline: OfflineConfiguration,

    // ACE: MasterConfiguration.DDD
    /// `DDD`.
    #[serde(rename = "DDD")]
    pub ddd: DDDConfiguration,

    /// Not ACE: the era the world plays (`[era]`).
    /// DIVERGE: an extra section; ACE has one set of rules.
    #[serde(rename = "Era")]
    pub era: EraConfiguration,
    /// `[portal_storms]`.
    #[serde(rename = "PortalStorms")]
    pub portal_storms: PortalStormConfiguration,

    /// Not ACE: the world's data overlay over the base data files (`[dat_overlay]`).
    /// DIVERGE: an extra section; ACE serves whole data files.
    #[serde(rename = "DatOverlay")]
    pub dat_overlay: DatOverlayConfiguration,

    /// Not ACE: the status ping on the game port (`[status]`).
    /// DIVERGE: an extra section; ACE answers nothing about a world without a login (V442).
    #[serde(rename = "Status")]
    pub status: StatusConfiguration,

    /// Not ACE: the Dereth Network, this world's link to other worlds (`[federation]`).
    /// DIVERGE: an extra section; ACE has no network of worlds (V451).
    #[serde(rename = "Federation")]
    pub federation: FederationConfiguration,

    /// Not ACE: server-controlled AI Players (`[ai_players]`).
    #[serde(rename = "AiPlayers")]
    pub ai_players: AiPlayersConfiguration,
}

/// `empyrean.toml` -> `[ai_players]`: how AI Players fill the world. Switched on and off with
/// `@aiplayers` (the `ai_players_enabled` property), not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiPlayersConfiguration {
    /// Seconds an AI Player stays once no real player is near.
    #[serde(rename = "LeaveAfterSeconds")]
    pub leave_after_seconds: u32,
    /// The most AI Players in the world at once.
    #[serde(rename = "MaxTotal")]
    pub max_total: u32,
    /// Town levels over the ones worked out from the land, as `Holtburg=5, Arwic=12`.
    #[serde(rename = "TownLevels")]
    pub town_levels: String,
}

impl Default for AiPlayersConfiguration {
    fn default() -> Self {
        Self {
            leave_after_seconds: 60,
            max_total: 60,
            town_levels: String::new(),
        }
    }
}

/// `empyrean.toml` -> `[portal_storms]`: moving players out of a crowded landblock. Off by default,
/// on any era.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortalStormConfiguration {
    /// How many players a landblock's surface holds before a storm brews. 0: no storms.
    #[serde(rename = "Threshold")]
    pub threshold: u32,
    /// How long a storm brews before it is imminent.
    #[serde(rename = "BrewSeconds")]
    pub brew_seconds: u32,
    /// How long a storm is imminent before it starts moving players.
    #[serde(rename = "ImminentSeconds")]
    pub imminent_seconds: u32,
}

impl Default for PortalStormConfiguration {
    fn default() -> Self {
        Self {
            threshold: 0,
            brew_seconds: 60,
            imminent_seconds: 60,
        }
    }
}
