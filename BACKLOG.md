# Feature Backlog

This is a list of outstanding feature and fixes. As they are handled, remove
them from this document and remember to document them in simple middle school
English in the README.md

When writing code: Only keep necessary comments, and keep them brief. No narrative, just
explanations and gotchas. 

Use this Markdown as guidance for how to code:
https://github.com/DietrichGebert/ponytail/blob/main/skills/ponytail/SKILL.md

Always use web search and read source. Do not trust any understanding of code
and APIS you have not sourced online/in the filesystem first.

# Issues

* For all the library download sources, see if you can figure out how to pull
  SMK-37 firmwares too, make sure they are correctly categorized.
* I'd like to be able to filter on device type as a first-class citizen: next to
  the search input bar I want a combo button with two toggles for FM-1 and
  SMK-37. FM-1 is on y default, SMK-37 is off by default, remember the toggles
  in the user's settings.
* For favoriting I want to go beyond stars -- I want to be able to favorite and
  assign a glyph. The default is ★ for star for favorite, but I also want 🗑
  (the non-emjoi wastebasket) and 𓅓 (Owl), 𓅃 (Falcon), 𓃱 (Giraffe). Next to the
  start toggle on the library tab I want a small discluure traingle button that
  pops up a menu allowing the user to select the firmware's custom glyph.
  custom glyph. When an item is given the wastebasket glyph, Simialrly to how
  a favorite sorts to top, will sort to bottom and be displayed in a slightly
  greyed-out manner. The favorite button will show the selected glyph,
  defaulting to star. This also must persist like favorites. When adding the
  star glyph to the tags, for a favorites, make it display the custom glyph.
  Unfaovrited items should still show the hollow star in their list items tag
  list to indicate they are not favorites.
