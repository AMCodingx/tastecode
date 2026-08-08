use crate::theme::{Theme, ThemeMode};
use crate::zoom::px;
use gpui::{
    AnyElement, Background, BoxShadow, Hsla, div, linear_color_stop, linear_gradient, point,
    prelude::*,
};

pub(crate) fn raised(theme: Theme) -> Background {
    gradient(theme, 0x242424, 0x1b1b1b, 0xffffff, 0xfafafa)
}

pub(crate) fn raised_hover(theme: Theme) -> Background {
    gradient(theme, 0x2c2c2c, 0x222222, 0xffffff, 0xf5f5f6)
}

/// The web `--bg-rail` token stays flat in dark mode and aliases the raised
/// chrome gradient in light mode.
pub(crate) fn rail_background(theme: Theme) -> Background {
    match theme.mode {
        ThemeMode::Dark => theme.rail.hsla().into(),
        ThemeMode::Light => raised(theme),
    }
}

/// The settings titlebar uses its dedicated dark color and the raised chrome
/// gradient in light mode, matching `--titlebar-bg`.
pub(crate) fn titlebar_background(theme: Theme) -> Background {
    match theme.mode {
        ThemeMode::Dark => theme.titlebar.hsla().into(),
        ThemeMode::Light => raised(theme),
    }
}

pub(crate) fn recessed(theme: Theme) -> Hsla {
    match theme.mode {
        ThemeMode::Dark => gpui::rgb(0x101010).into(),
        ThemeMode::Light => gpui::rgb(0xf3f3f5).into(),
    }
}

pub(crate) fn border(theme: Theme) -> Hsla {
    match theme.mode {
        ThemeMode::Dark => gpui::rgb(0x303030).into(),
        ThemeMode::Light => gpui::rgb(0xe3e3e6).into(),
    }
}

pub(crate) fn hover_border(theme: Theme) -> Hsla {
    border(theme).blend(theme.text_3.hsla().opacity(0.4))
}

pub(crate) fn menu_background(theme: Theme) -> Background {
    match theme.mode {
        ThemeMode::Dark => theme.surface_2.hsla().into(),
        ThemeMode::Light => raised(theme),
    }
}

pub(crate) fn menu_hover_background(theme: Theme) -> Background {
    match theme.mode {
        ThemeMode::Dark => theme.surface_3.hsla().into(),
        ThemeMode::Light => raised_hover(theme),
    }
}

pub(crate) fn menu_border(theme: Theme) -> Hsla {
    match theme.mode {
        ThemeMode::Dark => theme.line_strong.hsla(),
        ThemeMode::Light => gpui::rgb(0xe6e6e9).into(),
    }
}

pub(crate) fn flyout_shadows(theme: Theme) -> Vec<BoxShadow> {
    match theme.mode {
        ThemeMode::Dark => vec![BoxShadow {
            color: gpui::black().opacity(0.46),
            offset: point(px(0.0), px(8.0)),
            blur_radius: px(24.0),
            spread_radius: px(-14.0),
        }],
        ThemeMode::Light => vec![
            BoxShadow {
                color: gpui::rgba(0x18181b0d).into(),
                offset: point(px(0.0), px(1.0)),
                blur_radius: px(2.0),
                spread_radius: px(0.0),
            },
            BoxShadow {
                color: gpui::rgba(0x18181b2e).into(),
                offset: point(px(0.0), px(10.0)),
                blur_radius: px(28.0),
                spread_radius: px(-18.0),
            },
        ],
    }
}

pub(crate) fn shadows(theme: Theme) -> Vec<BoxShadow> {
    match theme.mode {
        ThemeMode::Dark => vec![BoxShadow {
            color: gpui::black().opacity(0.28),
            offset: point(px(0.0), px(1.0)),
            blur_radius: px(2.0),
            spread_radius: px(0.0),
        }],
        ThemeMode::Light => vec![
            BoxShadow {
                color: gpui::rgba(0x18181b14).into(),
                offset: point(px(0.0), px(1.0)),
                blur_radius: px(2.0),
                spread_radius: px(0.0),
            },
            BoxShadow {
                color: gpui::rgba(0x18181b29).into(),
                offset: point(px(0.0), px(4.0)),
                blur_radius: px(10.0),
                spread_radius: px(-8.0),
            },
        ],
    }
}

pub(crate) fn modal_shadows(theme: Theme) -> Vec<BoxShadow> {
    match theme.mode {
        ThemeMode::Dark => vec![BoxShadow {
            color: gpui::black().opacity(0.52),
            offset: point(px(0.0), px(20.0)),
            blur_radius: px(56.0),
            spread_radius: px(-24.0),
        }],
        ThemeMode::Light => vec![
            BoxShadow {
                color: gpui::rgba(0x18181b12).into(),
                offset: point(px(0.0), px(2.0)),
                blur_radius: px(6.0),
                spread_radius: px(0.0),
            },
            BoxShadow {
                color: gpui::rgba(0x18181b3d).into(),
                offset: point(px(0.0), px(24.0)),
                blur_radius: px(56.0),
                spread_radius: px(-30.0),
            },
        ],
    }
}

pub(crate) fn panel_shadows(theme: Theme) -> Vec<BoxShadow> {
    let mut result = shadows(theme);
    result.extend(modal_shadows(theme));
    result
}

pub(crate) fn top_highlight(theme: Theme) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(1.0))
        .bg(match theme.mode {
            ThemeMode::Dark => gpui::white().opacity(0.05),
            ThemeMode::Light => gpui::white().opacity(0.96),
        })
        .into_any_element()
}

pub(crate) fn right_highlight(theme: Theme) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .w(px(1.0))
        .bg(match theme.mode {
            ThemeMode::Dark => gpui::white().opacity(0.05),
            ThemeMode::Light => gpui::white().opacity(0.96),
        })
        .into_any_element()
}

pub(crate) fn inset_top_shade(theme: Theme) -> AnyElement {
    let (from, to): (Hsla, Hsla) = match theme.mode {
        ThemeMode::Dark => (gpui::black().opacity(0.34), gpui::transparent_black()),
        ThemeMode::Light => (gpui::rgba(0x18181b14).into(), gpui::transparent_black()),
    };
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(3.0))
        .bg(linear_gradient(
            180.0,
            linear_color_stop(from, 0.0),
            linear_color_stop(to, 1.0),
        ))
        .into_any_element()
}

fn gradient(
    theme: Theme,
    dark_from: u32,
    dark_to: u32,
    light_from: u32,
    light_to: u32,
) -> Background {
    let (from, to): (Hsla, Hsla) = match theme.mode {
        ThemeMode::Dark => (gpui::rgb(dark_from).into(), gpui::rgb(dark_to).into()),
        ThemeMode::Light => (gpui::rgb(light_from).into(), gpui::rgb(light_to).into()),
    };
    linear_gradient(
        180.0,
        linear_color_stop(from, 0.0),
        linear_color_stop(to, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rail_background_matches_the_theme_specific_web_token() {
        let dark = Theme::dark();
        let light = Theme::light();

        assert_eq!(rail_background(dark), dark.rail.hsla().into());
        assert_eq!(rail_background(light), raised(light));
    }

    #[test]
    fn titlebar_background_matches_the_theme_specific_web_token() {
        let dark = Theme::dark();
        let light = Theme::light();

        assert_eq!(titlebar_background(dark), dark.titlebar.hsla().into());
        assert_eq!(titlebar_background(light), raised(light));
    }
}
