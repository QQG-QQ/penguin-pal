// The original assistant remains available only through an explicit legacy build.
#[cfg(feature = "legacy-assistant")]
include!("lib.rs");

#[cfg(not(feature = "legacy-assistant"))]
mod companion;
#[cfg(not(feature = "legacy-assistant"))]
mod companion_data;
#[cfg(not(feature = "legacy-assistant"))]
mod companion_media;
#[cfg(not(feature = "legacy-assistant"))]
mod companion_platform;
#[cfg(not(feature = "legacy-assistant"))]
mod companion_runtime;
#[cfg(not(feature = "legacy-assistant"))]
mod companion_updates;
#[cfg(not(feature = "legacy-assistant"))]
pub use companion::run;
