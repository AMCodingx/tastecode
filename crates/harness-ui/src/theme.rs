use gpui::{Hsla, rgb};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const RAIL_WIDTH: f32 = 248.0;
pub const CHAT_WIDTH: f32 = 808.0;
pub const TITLEBAR_HEIGHT: f32 = 34.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    pub press: Duration,
    pub fast: Duration,
    pub slow: Duration,
}

impl Motion {
    pub const WEB_PARITY: Self = Self {
        press: Duration::from_millis(140),
        fast: Duration::from_millis(180),
        slow: Duration::from_millis(260),
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorToken(pub u32);

impl ColorToken {
    pub fn hsla(self) -> Hsla {
        rgb(self.0).into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backdrop {
    Default,
    Slate,
    Mocha,
    Forest,
    Midnight,
    Plum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Neutral,
    Ocean,
    Forest,
    Sunset,
    Amber,
    Rose,
    Lavender,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub mode: ThemeMode,
    pub background: ColorToken,
    pub rail: ColorToken,
    pub prompt: ColorToken,
    pub surface: ColorToken,
    pub surface_2: ColorToken,
    pub surface_3: ColorToken,
    pub queue_background: ColorToken,
    pub queue_line: ColorToken,
    pub queue_text: ColorToken,
    pub queue_action: ColorToken,
    pub queue_hover: ColorToken,
    pub line: ColorToken,
    pub line_strong: ColorToken,
    pub text: ColorToken,
    pub response_text: ColorToken,
    pub text_2: ColorToken,
    pub text_3: ColorToken,
    pub titlebar: ColorToken,
    pub titlebar_symbol: ColorToken,
    pub attention: ColorToken,
    pub error: ColorToken,
    pub success: ColorToken,
    pub motion: Motion,
}

impl Theme {
    pub fn new(mode: ThemeMode, backdrop: Backdrop, accent: Accent) -> Self {
        let mut theme = match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
        };
        theme.apply_backdrop(backdrop);
        theme.apply_accent(accent);
        theme
    }

    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            background: ColorToken(0x0f0f0f),
            rail: ColorToken(0x131313),
            prompt: ColorToken(0x1a1a1a),
            surface: ColorToken(0x1a1a1a),
            surface_2: ColorToken(0x222222),
            surface_3: ColorToken(0x2b2b2b),
            queue_background: ColorToken(0x222222),
            queue_line: ColorToken(0x2b2b2b),
            queue_text: ColorToken(0xededed),
            queue_action: ColorToken(0x8a8a8a),
            queue_hover: ColorToken(0x2b2b2b),
            line: ColorToken(0x262626),
            line_strong: ColorToken(0x333333),
            text: ColorToken(0xededed),
            response_text: ColorToken(0xfefefe),
            text_2: ColorToken(0xa3a3a3),
            text_3: ColorToken(0x6f6f6f),
            titlebar: ColorToken(0x151515),
            titlebar_symbol: ColorToken(0xf4f4f5),
            attention: ColorToken(0x4c9dff),
            error: ColorToken(0xe5687a),
            success: ColorToken(0x6fbf8e),
            motion: Motion::WEB_PARITY,
        }
    }

    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            background: ColorToken(0xfdfdfd),
            rail: ColorToken(0xffffff),
            prompt: ColorToken(0xffffff),
            surface: ColorToken(0xfafafa),
            surface_2: ColorToken(0xf5f5f6),
            surface_3: ColorToken(0xececef),
            queue_background: ColorToken(0xffffff),
            queue_line: ColorToken(0xeeeeef),
            queue_text: ColorToken(0x18181b),
            queue_action: ColorToken(0x96969a),
            queue_hover: ColorToken(0xf5f5f6),
            line: ColorToken(0xebebed),
            line_strong: ColorToken(0xdedee2),
            text: ColorToken(0x27272a),
            response_text: ColorToken(0x18181b),
            text_2: ColorToken(0x52525b),
            text_3: ColorToken(0x71717a),
            titlebar: ColorToken(0xffffff),
            titlebar_symbol: ColorToken(0x27272a),
            attention: ColorToken(0x2563eb),
            error: ColorToken(0xbe123c),
            success: ColorToken(0x16803c),
            motion: Motion::WEB_PARITY,
        }
    }

    fn apply_backdrop(&mut self, backdrop: Backdrop) {
        let colors = match (self.mode, backdrop) {
            (_, Backdrop::Default) => return,
            (ThemeMode::Dark, Backdrop::Slate) => {
                [0x0e1013, 0x12151a, 0x191d24, 0x20242c, 0x292e37]
            }
            (ThemeMode::Dark, Backdrop::Mocha) => {
                [0x121010, 0x161312, 0x1c1817, 0x241f1d, 0x2d2725]
            }
            (ThemeMode::Dark, Backdrop::Forest) => {
                [0x0e120f, 0x121713, 0x181f1a, 0x202822, 0x29322b]
            }
            (ThemeMode::Dark, Backdrop::Midnight) => {
                [0x0b0d14, 0x0e1119, 0x141828, 0x1b2030, 0x232939]
            }
            (ThemeMode::Dark, Backdrop::Plum) => [0x120f13, 0x161217, 0x1d181e, 0x251f26, 0x2e2730],
            (ThemeMode::Light, Backdrop::Slate) => {
                [0xf7f9fc, 0xf2f5f9, 0xeef2f7, 0xe3e9f1, 0xd7dfe9]
            }
            (ThemeMode::Light, Backdrop::Mocha) => {
                [0xfcfaf8, 0xf7f4f0, 0xf4f0eb, 0xebe5de, 0xe0d9d0]
            }
            (ThemeMode::Light, Backdrop::Forest) => {
                [0xf8fbf8, 0xf2f7f2, 0xeef4ee, 0xe3ece3, 0xd6e2d6]
            }
            (ThemeMode::Light, Backdrop::Midnight) => {
                [0xf5f7fc, 0xeff2f9, 0xebeef7, 0xdfe4f0, 0xd2d9e8]
            }
            (ThemeMode::Light, Backdrop::Plum) => {
                [0xfbf8fc, 0xf6f2f7, 0xf3eef4, 0xeae2ec, 0xded4e0]
            }
        };
        self.background = ColorToken(colors[0]);
        self.rail = ColorToken(colors[1]);
        self.prompt = ColorToken(colors[2]);
        self.surface = ColorToken(colors[2]);
        self.surface_2 = ColorToken(colors[3]);
        self.surface_3 = ColorToken(colors[4]);
    }

    fn apply_accent(&mut self, accent: Accent) {
        self.attention = ColorToken(match (self.mode, accent) {
            (ThemeMode::Dark, Accent::Neutral) => 0x4c9dff,
            (ThemeMode::Dark, Accent::Ocean) => 0x65b8ff,
            (ThemeMode::Dark, Accent::Forest) => 0x71c695,
            (ThemeMode::Dark, Accent::Sunset) => 0xc69cff,
            (ThemeMode::Dark, Accent::Amber) => 0xe0ad61,
            (ThemeMode::Dark, Accent::Rose) => 0xe593ad,
            (ThemeMode::Dark, Accent::Lavender) => 0xae9eea,
            (ThemeMode::Light, Accent::Neutral) => 0x2563eb,
            (ThemeMode::Light, Accent::Ocean) => 0x1677be,
            (ThemeMode::Light, Accent::Forest) => 0x247b4f,
            (ThemeMode::Light, Accent::Sunset) => 0x7e4bc2,
            (ThemeMode::Light, Accent::Amber) => 0x936017,
            (ThemeMode::Light, Accent::Rose) => 0xa34264,
            (ThemeMode::Light, Accent::Lavender) => 0x6653aa,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_geometry_and_motion_match_the_css_contract() {
        assert_eq!(RAIL_WIDTH, 248.0);
        assert_eq!(CHAT_WIDTH, 808.0);
        assert_eq!(TITLEBAR_HEIGHT, 34.0);
        assert_eq!(Motion::WEB_PARITY.press, Duration::from_millis(140));
        assert_eq!(Motion::WEB_PARITY.fast, Duration::from_millis(180));
        assert_eq!(Motion::WEB_PARITY.slow, Duration::from_millis(260));
    }

    #[test]
    fn neutral_theme_tokens_match_the_web_oracle() {
        let dark = Theme::dark();
        let light = Theme::light();

        assert_eq!(dark.background, ColorToken(0x0f0f0f));
        assert_eq!(dark.rail, ColorToken(0x131313));
        assert_eq!(dark.text, ColorToken(0xededed));
        assert_eq!(dark.queue_background, ColorToken(0x222222));
        assert_eq!(dark.queue_line, ColorToken(0x2b2b2b));
        assert_eq!(dark.queue_action, ColorToken(0x8a8a8a));
        assert_eq!(light.background, ColorToken(0xfdfdfd));
        assert_eq!(light.text, ColorToken(0x27272a));
        assert_eq!(light.line, ColorToken(0xebebed));
        assert_eq!(light.queue_background, ColorToken(0xffffff));
        assert_eq!(light.queue_line, ColorToken(0xeeeeef));
        assert_eq!(light.queue_action, ColorToken(0x96969a));
    }

    #[test]
    fn every_backdrop_and_accent_has_a_distinct_typed_variant() {
        let slate = Theme::new(ThemeMode::Dark, Backdrop::Slate, Accent::Ocean);
        let plum = Theme::new(ThemeMode::Light, Backdrop::Plum, Accent::Lavender);

        assert_eq!(slate.background, ColorToken(0x0e1013));
        assert_eq!(slate.attention, ColorToken(0x65b8ff));
        assert_eq!(plum.background, ColorToken(0xfbf8fc));
        assert_eq!(plum.attention, ColorToken(0x6653aa));
    }
}
