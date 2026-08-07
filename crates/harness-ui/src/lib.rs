mod accessibility;
mod app;
mod assets;
mod chat;
mod client_state;
mod preferences;
mod preview_capture;
mod shortcuts;
mod sidebar;
mod theme;
mod zoom;

pub use app::run;
pub use theme::{
    Accent, Backdrop, CHAT_WIDTH, ColorToken, Motion, RAIL_WIDTH, TITLEBAR_HEIGHT, Theme, ThemeMode,
};
