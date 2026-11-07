//! The Library tab: every firmware the app knows, as a grouped scrollable list —
//! installed on top (builtins, imported files, downloaded hub packages), the hub's
//! online offers below. Items remove, download and flash (a live unit via the blocking
//! dialog in [`crate::flash_dialog`], or a powered-off unpinned grid slot, which then
//! powers on). At the bottom, the add-a-file form.

use crate::app::{DeviceFilter, GridApp};
use fm1_performer::firmware_picker::{pick_of, FirmwarePick};
use fm1_performer::library::{Firmware, FirmwareDevice, FirmwareSource, Status};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{h_flex, v_flex, v_virtual_list, ElementExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};
use std::rc::Rc;

/// Tag pill colors are eight hue rotations of the theme's base colour, picked by
/// the tag's hash so the same tag always lands on the same hue. The base is the
/// chosen accent for Influenza/Meltysand and the theme's primary otherwise.
/// Saturation/lightness are clamped so every pill stays readable on light and dark
/// surfaces, and a neutral primary gets a blue hue to rotate.

mod actions;
mod favorites;
mod filter;
mod flash;
#[cfg(test)]
mod tests;

pub(crate) use filter::*;
