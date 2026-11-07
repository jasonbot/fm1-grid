use super::*;

impl GridApp {
    /// Toggle a firmware's favorite. Dropping it forgets its glyph too; the session
    /// is saved at once, like the theme pick — a crash shouldn't lose the toggle.
    pub(crate) fn toggle_favorite(&mut self, name: &str, cx: &App) {
        let key = favorite_key(name);
        if !self.favorites.insert(key.clone()) {
            self.favorites.remove(&key);
            self.favorite_glyphs.remove(&key);
        }
        self.favorites_rev = self.favorites_rev.wrapping_add(1);
        self.save_session(cx);
    }
    /// Give a firmware a favorite glyph, which also favorites it. ★ is the default
    /// and stores no override.
    pub(crate) fn set_favorite_glyph(&mut self, name: &str, glyph: &str, cx: &App) {
        let key = favorite_key(name);
        self.favorites.insert(key.clone());
        if glyph == FAV_TAG {
            self.favorite_glyphs.remove(&key);
        } else {
            self.favorite_glyphs.insert(key, glyph.to_string());
        }
        self.favorites_rev = self.favorites_rev.wrapping_add(1);
        self.save_session(cx);
    }
}
