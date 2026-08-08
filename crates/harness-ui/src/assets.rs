use anyhow::Result;
use gpui::{App, AssetSource, SharedString};
use std::borrow::Cow;

pub struct HarnessAssets;

impl AssetSource for HarnessAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/panel-left.svg" => Some(include_bytes!("../assets/icons/panel-left.svg")),
            "icons/plus.svg" => Some(include_bytes!("../assets/icons/plus.svg")),
            "icons/folder-pen.svg" => Some(include_bytes!("../assets/icons/folder-pen.svg")),
            "icons/folder-plus.svg" => Some(include_bytes!("../assets/icons/folder-plus.svg")),
            "icons/search.svg" => Some(include_bytes!("../assets/icons/search.svg")),
            "icons/settings.svg" => Some(include_bytes!("../assets/icons/settings.svg")),
            "icons/inbox.svg" => Some(include_bytes!("../assets/icons/inbox.svg")),
            "icons/chevron-right.svg" => Some(include_bytes!("../assets/icons/chevron-right.svg")),
            "icons/chevron-down.svg" => Some(include_bytes!("../assets/icons/chevron-down.svg")),
            "icons/chevron-up.svg" => Some(include_bytes!("../assets/icons/chevron-up.svg")),
            "icons/check.svg" => Some(include_bytes!("../assets/icons/check.svg")),
            "icons/check-check.svg" => Some(include_bytes!("../assets/icons/check-check.svg")),
            "icons/clock-3.svg" => Some(include_bytes!("../assets/icons/clock-3.svg")),
            "icons/ellipsis.svg" => Some(include_bytes!("../assets/icons/ellipsis.svg")),
            "icons/shield-question.svg" => {
                Some(include_bytes!("../assets/icons/shield-question.svg"))
            }
            "icons/shield-check.svg" => Some(include_bytes!("../assets/icons/shield-check.svg")),
            "icons/shield-alert.svg" => Some(include_bytes!("../assets/icons/shield-alert.svg")),
            "icons/scan-eye.svg" => Some(include_bytes!("../assets/icons/scan-eye.svg")),
            "icons/lock-open.svg" => Some(include_bytes!("../assets/icons/lock-open.svg")),
            "icons/lock-keyhole.svg" => Some(include_bytes!("../assets/icons/lock-keyhole.svg")),
            "icons/zap.svg" => Some(include_bytes!("../assets/icons/zap.svg")),
            "icons/zap-filled.svg" => Some(include_bytes!("../assets/icons/zap-filled.svg")),
            "icons/palette.svg" => Some(include_bytes!("../assets/icons/palette.svg")),
            "icons/mic.svg" => Some(include_bytes!("../assets/icons/mic.svg")),
            "icons/openai.svg" => Some(include_bytes!("../assets/icons/openai.svg")),
            "icons/anthropic.svg" => Some(include_bytes!("../assets/icons/anthropic.svg")),
            "icons/grok.svg" => Some(include_bytes!("../assets/icons/grok.svg")),
            "icons/cursor.svg" => Some(include_bytes!("../assets/icons/cursor.svg")),
            "icons/opencode.svg" => Some(include_bytes!("../assets/icons/opencode.svg")),
            "icons/openrouter.svg" => Some(include_bytes!("../assets/icons/openrouter.svg")),
            "icons/kimi.svg" => Some(include_bytes!("../assets/icons/kimi.svg")),
            "icons/qwen.svg" => Some(include_bytes!("../assets/icons/qwen.svg")),
            "icons/zai.svg" => Some(include_bytes!("../assets/icons/zai.svg")),
            "icons/antigravity.svg" => Some(include_bytes!("../assets/icons/antigravity.svg")),
            "icons/pi.svg" => Some(include_bytes!("../assets/icons/pi.svg")),
            "icons/acp.svg" => Some(include_bytes!("../assets/icons/acp.svg")),
            "icons/custom-provider.svg" => {
                Some(include_bytes!("../assets/icons/custom-provider.svg"))
            }
            "icons/file-diff.svg" => Some(include_bytes!("../assets/icons/file-diff.svg")),
            "icons/atom.svg" => Some(include_bytes!("../assets/icons/atom.svg")),
            "icons/hash.svg" => Some(include_bytes!("../assets/icons/hash.svg")),
            "icons/code-xml.svg" => Some(include_bytes!("../assets/icons/code-xml.svg")),
            "icons/braces.svg" => Some(include_bytes!("../assets/icons/braces.svg")),
            "icons/pilcrow.svg" => Some(include_bytes!("../assets/icons/pilcrow.svg")),
            "icons/file-text.svg" => Some(include_bytes!("../assets/icons/file-text.svg")),
            "icons/cog.svg" => Some(include_bytes!("../assets/icons/cog.svg")),
            "icons/gem.svg" => Some(include_bytes!("../assets/icons/gem.svg")),
            "icons/coffee.svg" => Some(include_bytes!("../assets/icons/coffee.svg")),
            "icons/bird.svg" => Some(include_bytes!("../assets/icons/bird.svg")),
            "icons/settings-2.svg" => Some(include_bytes!("../assets/icons/settings-2.svg")),
            "icons/container.svg" => Some(include_bytes!("../assets/icons/container.svg")),
            "icons/file-code-2.svg" => Some(include_bytes!("../assets/icons/file-code-2.svg")),
            "icons/rotate-ccw.svg" => Some(include_bytes!("../assets/icons/rotate-ccw.svg")),
            "icons/refresh-cw.svg" => Some(include_bytes!("../assets/icons/refresh-cw.svg")),
            "icons/copy.svg" => Some(include_bytes!("../assets/icons/copy.svg")),
            "icons/x.svg" => Some(include_bytes!("../assets/icons/x.svg")),
            "icons/arrow-left.svg" => Some(include_bytes!("../assets/icons/arrow-left.svg")),
            "icons/arrow-right.svg" => Some(include_bytes!("../assets/icons/arrow-right.svg")),
            "icons/arrow-up.svg" => Some(include_bytes!("../assets/icons/arrow-up.svg")),
            "icons/arrow-down.svg" => Some(include_bytes!("../assets/icons/arrow-down.svg")),
            "icons/square.svg" => Some(include_bytes!("../assets/icons/square.svg")),
            "icons/corner-down-right.svg" => {
                Some(include_bytes!("../assets/icons/corner-down-right.svg"))
            }
            "icons/resize-corner.svg" => Some(include_bytes!("../assets/icons/resize-corner.svg")),
            "icons/pencil.svg" => Some(include_bytes!("../assets/icons/pencil.svg")),
            "icons/pin.svg" => Some(include_bytes!("../assets/icons/pin.svg")),
            "icons/pin-off.svg" => Some(include_bytes!("../assets/icons/pin-off.svg")),
            "icons/archive.svg" => Some(include_bytes!("../assets/icons/archive.svg")),
            "icons/folder-open.svg" => Some(include_bytes!("../assets/icons/folder-open.svg")),
            "icons/panel-left-close.svg" => {
                Some(include_bytes!("../assets/icons/panel-left-close.svg"))
            }
            "icons/log-out.svg" => Some(include_bytes!("../assets/icons/log-out.svg")),
            "icons/square-pen.svg" => Some(include_bytes!("../assets/icons/square-pen.svg")),
            "icons/trash-2.svg" => Some(include_bytes!("../assets/icons/trash-2.svg")),
            "icons/blocks.svg" => Some(include_bytes!("../assets/icons/blocks.svg")),
            "icons/bell.svg" => Some(include_bytes!("../assets/icons/bell.svg")),
            "icons/boxes.svg" => Some(include_bytes!("../assets/icons/boxes.svg")),
            "icons/database.svg" => Some(include_bytes!("../assets/icons/database.svg")),
            "icons/info.svg" => Some(include_bytes!("../assets/icons/info.svg")),
            "icons/network.svg" => Some(include_bytes!("../assets/icons/network.svg")),
            "icons/user-round.svg" => Some(include_bytes!("../assets/icons/user-round.svg")),
            "icons/git-branch.svg" => Some(include_bytes!("../assets/icons/git-branch.svg")),
            "icons/history.svg" => Some(include_bytes!("../assets/icons/history.svg")),
            "icons/square-terminal.svg" => {
                Some(include_bytes!("../assets/icons/square-terminal.svg"))
            }
            "icons/folder.svg" => Some(include_bytes!("../assets/icons/folder.svg")),
            "icons/laptop.svg" => Some(include_bytes!("../assets/icons/laptop.svg")),
            "icons/gauge.svg" => Some(include_bytes!("../assets/icons/gauge.svg")),
            "icons/octagon-x.svg" => Some(include_bytes!("../assets/icons/octagon-x.svg")),
            "icons/panels-top-left.svg" => {
                Some(include_bytes!("../assets/icons/panels-top-left.svg"))
            }
            "icons/loader-circle.svg" => Some(include_bytes!("../assets/icons/loader-circle.svg")),
            "icons/attachment-loader.svg" => {
                Some(include_bytes!("../assets/icons/attachment-loader.svg"))
            }
            "icons/brain.svg" => Some(include_bytes!("../assets/icons/brain.svg")),
            "icons/file-pen-line.svg" => Some(include_bytes!("../assets/icons/file-pen-line.svg")),
            "icons/wrench.svg" => Some(include_bytes!("../assets/icons/wrench.svg")),
            "icons/book-open.svg" => Some(include_bytes!("../assets/icons/book-open.svg")),
            "icons/images.svg" => Some(include_bytes!("../assets/icons/images.svg")),
            "icons/list-checks.svg" => Some(include_bytes!("../assets/icons/list-checks.svg")),
            "icons/circle-alert.svg" => Some(include_bytes!("../assets/icons/circle-alert.svg")),
            "icons/triangle-alert.svg" => {
                Some(include_bytes!("../assets/icons/triangle-alert.svg"))
            }
            "icons/circle-question-mark.svg" => {
                Some(include_bytes!("../assets/icons/circle-question-mark.svg"))
            }
            "icons/file.svg" => Some(include_bytes!("../assets/icons/file.svg")),
            "icons/image.svg" => Some(include_bytes!("../assets/icons/image.svg")),
            "icons/download.svg" => Some(include_bytes!("../assets/icons/download.svg")),
            "icons/maximize-2.svg" => Some(include_bytes!("../assets/icons/maximize-2.svg")),
            "icons/minus.svg" => Some(include_bytes!("../assets/icons/minus.svg")),
            _ => None,
        };
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

pub fn register_fonts(cx: &mut App) -> Result<()> {
    cx.text_system().add_fonts(vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-400.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-500.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-520.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-530.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-540.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-550.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-560.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-570.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-580.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-600.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-680.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-700.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-400.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-500.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-520.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-530.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-540.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-550.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-560.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-570.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-580.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-600.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-680.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-700.ttf")),
    ])?;
    Ok(())
}
