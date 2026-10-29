# Feature Backlog

This is a list of outstanding feature and fixes. As they are handled, remove
them fro mthis document and remember to document them in simple middle school
English in the README.md

When writing code: Only keep necessary comments, and keep them brief. No narrative, just
explanations and gotchas. 

Use this Markdown as guidance for how to code: https://github.com/DietrichGebert/ponytail/blob/main/skills/ponytail/SKILL.md

always use web serach and read source. Do not trust any understanding of code
and APIS you have not sourced online/in the filesystem first.

## Split up Devices tab

The devices tab currently has two sections (Devices and MIDI Graph) as a split pane. I want this to be
split into two tab sections instead, with the same labels. The tab bar should be
on top of the display area, and be a Pill Tab. As the selected main tab is saved
as state when the app closes, save this state too. Add icons to each tab pill,
and remember to add references to the appropraite icon name in `app.rs` so the
icons actually draw.

## Make Devices Tab sections scrollable

Both subsections in the devices tab should be given the same treatment as the
library tab and the items should be in a scrollable area which is repsonsive to
pageup/pagedown and friends.

## MIDI Rescue as an Optional UI component

Right now "MIDI Rescue" shows up if no detected devices are found. I would like
instead for there to be a 'Rescue' toggle button that allows non-FM-1 to be
flashed in addition to the recognized FM-1 devices, and if no FM-` devices are
found but a MIDI device is, I want this toggle to have a warning ring.

## Device list as a more prominent UI feature

Right now each FM-1 device in the pane uses very little space, it can occopy
more. Increase the size of the font for the device name, move the flash toggle
and selector to its own row.

## MIDI Graph as a better UI

The MIDI Graph is currently small and clunky. Instead, in this larger space, I
want it to look like this

INPUT 1
Device it is routed to 1    x
Device it is routed to 2    x
[ Connect | v ]

INPUT 2
[ Connext | v ]

INPUT 3
Device it is routed to 1    x
[ Connect | v ]

Each device has its own section that always displays, there is an always-there
router dropdown, and between the two are all the midi devices it gets divertex
to with an (X) pill button to unroute it.

## Emulator sidebar changes

Move the bottom bar with the power / firmware name slector / pin buttons to the
top of the pane. Make the buttons larger (maybe large). Replace the power switch with a toggle 
button with an appropriate power icon. Make the firmware name/firmware selector
element flex grow horizontal so the row takes up the whole width of the sidebar.
