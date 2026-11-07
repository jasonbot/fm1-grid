use super::*;

impl GridApp {
    /// Apply the theme family and appearance override: swap the family's configs
    /// into the registry's light/dark slots, then load the side the mode points at.
    /// `Theme::change` reloads the current mode's config, so this repaints at once.
    /// Influenza has no registry entry: both sides are computed from the accent.
    pub(crate) fn apply_theme(&self, cx: &mut Context<Self>) {
        let family = crate::theme::family(&self.theme_id);
        let accent = crate::themes::accent(&self.accent);
        let (light, dark) = match family.id {
            crate::theme::INFLUENZA => (
                Rc::new(crate::themes::influenza::theme(ThemeMode::Light, accent)),
                Rc::new(crate::themes::influenza::theme(ThemeMode::Dark, accent)),
            ),
            crate::theme::MELTYSAND => (
                Rc::new(crate::themes::meltysand::theme(ThemeMode::Light, accent)),
                Rc::new(crate::themes::meltysand::theme(ThemeMode::Dark, accent)),
            ),
            crate::theme::VISUALIA => (
                Rc::new(crate::themes::visualia::theme(ThemeMode::Light, accent)),
                Rc::new(crate::themes::visualia::theme(ThemeMode::Dark, accent)),
            ),
            _ => {
                let registry = ThemeRegistry::global(cx);
                let pick = |name: &Option<String>,
                            fallback: &Rc<gpui_kit::component::ThemeConfig>| {
                    name.as_ref()
                        .and_then(|name| registry.themes().get(name.as_str()).cloned())
                        .unwrap_or_else(|| fallback.clone())
                };
                (
                    pick(&family.light, registry.default_light_theme()),
                    pick(&family.dark, registry.default_dark_theme()),
                )
            }
        };
        // A theme's font fields are ordered lists; name the first family this
        // machine actually has (the registry configs are single-name strings).
        let installed = crate::theme::installed_fonts(cx);
        let light = Rc::new(crate::theme::resolve_theme_fonts(&light, installed));
        let dark = Rc::new(crate::theme::resolve_theme_fonts(&dark, installed));
        let mode = match self.appearance {
            Appearance::Light => ThemeMode::Light,
            Appearance::Dark => ThemeMode::Dark,
            Appearance::Auto => cx.window_appearance().into(),
        };
        {
            let theme = Theme::global_mut(cx);
            theme.light_theme = light;
            theme.dark_theme = dark;
        }
        Theme::change(mode, None, cx);
    }
    /// The Settings tab's theme pick: apply at once and save the session record
    /// immediately (a crash shouldn't lose the choice; the quit hook saves again).
    pub(crate) fn set_theme_family(&mut self, id: &str, cx: &mut Context<Self>) {
        self.theme_id = id.to_string();
        self.apply_theme(cx);
        self.save_session(cx);
        cx.notify();
    }
    /// The Settings tab's accent pick: only Influenza consumes it, but the choice
    /// is stored for every family.
    pub(crate) fn set_accent(&mut self, id: &str, cx: &mut Context<Self>) {
        self.accent = crate::themes::accent(id).id.to_string();
        self.apply_theme(cx);
        self.save_session(cx);
        cx.notify();
    }
    /// The Settings tab's appearance pick: apply at once and save.
    pub(crate) fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        self.appearance = appearance;
        self.apply_theme(cx);
        self.save_session(cx);
        cx.notify();
    }
}
