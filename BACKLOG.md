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

* The pin button looks silly so large on the emulator sidebar, make it medium
  again.
* The pill tabs on the devices view sohuld have captions. Youu may need to
  replace the contents of each tab with a ghost button with an icon to get
  text and icon at the same time.
* I would like to add a settings view to the main tab bar pane. The settings
  pane, for now, should just have a theme picker dropdown. For the time being the themes
  available should be the ones built in to gpui kit: https://github.com/longbridge/gpui-kit/tree/main/themes
  Everything you set on this page should be saved in app settings. The theme,
  then selected, should change immediately. A combo auto/light/dark selector to
  let the user override if they want light or dark shoudl be there too. Use
  icons. Remember to add references to the min `app.rs` so they render.
