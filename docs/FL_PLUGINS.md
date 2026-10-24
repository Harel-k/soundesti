# Using FL Studio's own plugins in Soundesti

_Investigated 2026-10-24 on a PC with FL Studio 2026 installed._

## Short answer

You **can** use FL Studio's instruments and effects (Sytrus, Harmor, FLEX, Gross Beat,
Fruity effects…) inside Soundesti through **FL Studio VSTi**, the plugin version of FL
Studio that Image-Line ships with FL Studio for exactly this purpose (using FL inside
other music programs). It needs your own FL Studio installation and license.

Soundesti **cannot** load FL's native plugin files directly, and it cannot recreate the
sound of an FL plugin from a `.flp` project without that plugin.

## How to do it

1. Install FL Studio (FL's installer also puts `FL Studio VSTi.dll` and
   `FL Studio VSTi (Multi).dll` in `C:\Program Files\Common Files\VST2`).
2. In Soundesti: **OPTIONS › Manage plugins › Find plugins** (Soundesti also looks in
   FL's own `System\Plugin\VSTi\x64` folder).
3. **ADD › Plugins › FL Studio VSTi** (or the Channel Rack's "+ Add").
4. In the FL Studio VSTi window, click the fruit logo: the full FL Studio opens inside it,
   where you can load any FL instrument or effect. Notes from Soundesti's Channel Rack,
   Piano roll and Playlist play it; tempo and position follow Soundesti.
5. Saving the Soundesti project saves FL Studio VSTi's state with it.

Use **FL Studio VSTi (Multi)** to get several separate outputs (Soundesti currently uses
the first stereo pair).

### Verified

- Scan finds both FL Studio VSTi variants without starting FL (no pop-ups during scans).
- Loading, notes, transport, the FL Studio VSTi window, saving/restoring its state,
  removing the channel and quitting Soundesti all work (manual checks, 2026-10-24).
- On first load FL may show its own messages (for example a "Cloud path warning" when
  your FL user data folder is inside OneDrive). That is FL Studio talking, not Soundesti;
  answer it once with "Remember my choice".
- FL Studio VSTi crashes inside its own code if its DLL is unloaded while the program is
  exiting; Soundesti therefore exits without unloading plugin code (see KNOWN_ISSUES.md,
  KI-PLUGIN-1).

## Why not load FL's plugins directly?

FL's built-in plugins live in `FL Studio\Plugins\Fruity\Generators` and `…\Effects` as
`*_x64.dll` files in FL Studio's **own plugin format** (not VST/CLAP). Hosting them would
mean reimplementing FL Studio's host interface, and:

- they rely heavily on services only FL Studio provides (its browser, sample loading,
  user data, licensing and unlock checks);
- they are licensed for use inside FL Studio; Image-Line's supported way to use them
  elsewhere is FL Studio VSTi (above) or the plugins Image-Line sells separately in
  VST/VST3 form;
- a partial, unofficial host would be fragile across FL updates.

So Soundesti does not attempt it.

## Why can't an imported `.flp` keep the plugin sounds?

Each FL plugin stores its settings in the project as a private binary blob whose format
only that plugin understands. The `.flp` importer therefore brings across everything
that is documented and plugin-independent (tempo, channels, patterns, notes, Playlist,
samples) and replaces FL plugins with the closest Soundesti instrument, listing each
substitution in the import report.

For the real FL sound in a Soundesti project: add FL Studio VSTi and load the same FL
plugin inside it, or render the part to audio in FL Studio and use the WAV in a Soundesti
Sampler channel.

## Image-Line plugins sold as VST3

Image-Line also sells some of its plugins as standalone VST/VST3 plugins for other
programs. Those behave like any other VST3 in Soundesti (scan, add, open window, save).
