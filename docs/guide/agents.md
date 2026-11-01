# AI agents

An agent that can only read your code has to guess what your addon looks like
and does. `wowlua mcp` gives it the emulated WoW client you test and debug
in: it can start your addon, look at the screen, click and type, fire events,
set breakpoints and profile, then check, format and test its own fix, with no
game client.

## Connect your agent

`wowlua mcp` is a Model Context Protocol server on stdio. It works on the
package it is started in, so register it for your addon's folder, the one
with `wowlua.json`. With Claude Code, run this in that folder:

```console
$ claude mcp add wowlua -- wowlua mcp
```

Any other MCP client runs the same command. In a client configured with JSON,
the server entry looks like this:

```json
{
  "mcpServers": {
    "wowlua": { "command": "wowlua", "args": ["mcp"] }
  }
}
```

> [!NOTE]
> The agent works on the same emulated client as `wowlua test` and the
> [VS Code debugger](vscode.md#debug-in-the-emulated-client). With your WoW
> installation at hand, its screenshots are drawn with the game's own
> textures and fonts, read on your machine and never copied anywhere.

## What a session looks like

Ask for an outcome, not for tool calls. Say a label in your options window is
cut off, and you ask the agent to fix it and show you the result. A session
goes like this:

1. **Start the addon** The agent starts your addon, logged in, in the flavor
   and world profile you work with. `start_session`
2. **Open the window** It runs the slash command that opens it.
   `slash`
3. **Look** It takes a screenshot of the window and reads the frame tree,
   which gives each frame's size and text. `screenshot`
4. **Fix the code** It changes the anchor or width of the label in your
   source. Then it starts a new session and opens the window again.
   `start_session`
5. **Check** A second screenshot shows the label in full, and the agent
   writes a screenshot test so it stays that way. `screenshot`

## What the agent can do

The tools follow how you work on an addon yourself. Each tool that runs code
answers once the game stops or settles, with where it stopped and what was
printed in the meantime.

### Run

`start_session` starts the addon, logged in, or one of your test files, with
an optional flavor, world profile, clock, and breakpoints to start with.
`stop_session` ends it.

### See

`screenshot` draws the screen or one frame and returns it as an image.
`frame_tree` lists frames with their type, name, shown and visible state,
rectangle and text, and `find_frames` finds frames by name, type or text.
`pick` tells which frame draws at a point of the screenshot, and `inspect`
gives one frame's details: its strata and level, anchors, scripts and hooks
with where their functions are, registered events, regions, children, and
the line of your code that made it.

### The world

`units` lists the world's units with their name, reaction, health, distance,
place and whether the player sees them. `events` logs the game events told
to your addon, with their arguments, the frames they reached and how long
the handlers took. `set_screen` changes the window size and UI scale as the
Graphics settings do, and `save_world` gives the running world as a world
profile, to keep as `tests/profiles/<name>.json`.

### Act

`click` clicks a frame or a point of the screenshot, as the player would.
`hover`, `drag`, `type_text`, `press_key` and `slash` do the rest of what a
player does, and `fire_event` fires a documented game event. `world` changes
the running world with the controls tests use, such as
`mock.player:setMoney(500)` or `mock.player:enterCombat()`.

### Move time

`advance` moves the game's time on by seconds or frames, so timers,
`OnUpdate` and animations run exactly as far as the agent asks.

### Debug

`set_breakpoints` and `set_function_breakpoints` set line breakpoints and
breakpoints on functions, API calls, events, scripts and timers. `continue`,
`step` and `pause` control the run, `stack` and `variables` show where it
stopped, and `eval` runs Lua in the game or in the stopped function.
`breakpoints` lists them all with how often each was hit. A stop at an error,
a blocked action or taint comes with its details: the message and stack, or
the action and where the taint came from.

### Measure and read

`profile_start` and `profile_stop` profile the session, optionally writing a
speedscope file. `output` returns what the session printed and the errors it
raised.

### Check, format and test

These work on your package and need no session. `check` runs `wowlua check`
and answers with one line per problem, with its file, line and code. `fmt`
formats your sources, or with `check` only names the files it would change.
`build` builds the addon. `test` runs your tests, all of them or those whose
names contain a filter, and answers with how many passed and each failure
with its message and line; with `coverage` it adds the totals and the files
with the most lines that never ran.

## Why its results repeat

An agent's sessions run on a manual clock unless it asks for real time: game
time moves only when it calls `advance`, and it stands still while the
session is paused. The same steps give the same screen every time, so a
screenshot before and after a change is a fair comparison, and a bug the
agent found can become a test that fails the same way.

Profiles count Lua instructions as well as time. Instruction counts are the
same on every machine, so a claim such as "this change made `onBagUpdate`
40% cheaper" is one you can check.

## Things worth asking for

| Ask | What the agent does |
|-----|---------------------|
| "Open the options window and fix anything that overlaps." | Opens it with `slash` or `click`, takes a screenshot, finds the frames, edits the code, takes another screenshot |
| "Why does the counter stop after the first loot?" | Sets a breakpoint on the event, fires it with `fire_event`, steps and reads the variables |
| "Invite two party members and check the party frames." | Starts with a world profile that has other players, runs `/invite`, takes a screenshot |
| "Make the bag scan cheaper." | Profiles while it fires bag events, changes the code, compares the instruction counts |
| "Run the tests and fix what fails." | Runs `test`, reads the failures, fixes the code, runs `check` and `test` again |
| "Turn what you just checked into a test." | Writes a `@test` with `mock.slash`, `mock.click` and `toMatchScreenshot` |

## Keep what the agent checked

A session proves the fix once, and a test keeps it fixed. The tools map onto
`wowlua:mock`, so the agent can write down what it did as a test:

```wowlua
import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("the options window fits its labels")
local function labelsFit()
    mock.slash("/myaddon")
    expect(_G.MyAddonOptions).toMatchScreenshot("open")
end
```

[Testing your addon](testing.md) covers screenshots, world profiles and
running tests in CI, and [Debugging](debugging.md) the same debugger in your
editor. The tools and their arguments are in
[Debugger §18.12.2](../../spec/toolchain/debugger.md#18122-wowlua-mcp-model-context-protocol).
