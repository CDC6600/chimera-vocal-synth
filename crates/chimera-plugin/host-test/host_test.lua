-- CHIMERA in a real host. REAPER runs this at start-up (as Scripts/__startup.lua): it loads each
-- format, renders what each mode does to files for check.ts to judge, then opens each editor for
-- the script that started it, which can see and click (macos/host-test.sh on CI).
-- See docs/plugin.md, "Testing in a host".
--
-- Environment: CHIMERA_TEST_OUT (a folder), CHIMERA_TEST_FORMATS ("CLAP,VST3,AU"),
-- CHIMERA_TEST_EDITOR (1 to open the editors: Linux has none), CHIMERA_TEST_RATE (Hz).

local OUT = os.getenv("CHIMERA_TEST_OUT")
-- the rate REAPER runs its plugins at until it renders: rendering at another restarts a VST3 partway
-- through, because Beatbox's latency in samples changes (docs/plugin.md, "Testing in a host")
local FS, BPM = tonumber(os.getenv("CHIMERA_TEST_RATE") or "44100"), 120
local FORMATS = {}
for f in (os.getenv("CHIMERA_TEST_FORMATS") or "CLAP"):gmatch("[^,]+") do FORMATS[#FORMATS + 1] = f end

local results = io.open(OUT .. "/results.jsonl", "a")
local function json(v)
  if type(v) == "string" then return '"' .. v:gsub('[\\"]', "\\%0"):gsub("%c", " ") .. '"' end
  if type(v) ~= "table" then return tostring(v) end
  local parts = {}
  for k, x in pairs(v) do parts[#parts + 1] = json(k) .. ":" .. json(x) end
  return "{" .. table.concat(parts, ",") .. "}"
end
local function record(t)
  results:write(json(t), "\n")
  results:flush()
end
local function touch(name)
  io.open(OUT .. "/" .. name, "w"):close()
end

-- REAPER gives plugin state as base64, and has nothing to decode it with.
local B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local function b64_decode(s)
  local out, acc, bits = {}, 0, 0
  for c in s:gmatch("[%w+/]") do
    acc = acc << 6 | (B64:find(c, 1, true) - 1)
    bits = bits + 6
    if bits >= 8 then
      bits = bits - 8
      out[#out + 1] = string.char(acc >> bits & 255)
    end
  end
  return table.concat(out)
end
local function b64_encode(s)
  local out = {}
  for i = 1, #s, 3 do
    local a, b, c = s:byte(i, i + 2)
    local n = a << 16 | (b or 0) << 8 | (c or 0)
    local function at(shift)
      local k = (n >> shift & 63) + 1
      return B64:sub(k, k)
    end
    out[#out + 1] = at(18) .. at(12) .. (b and at(6) or "=") .. (c and at(0) or "=")
  end
  return table.concat(out)
end

local function chunk_name(format)
  return format == "CLAP" and "clap_chunk" or "vst_chunk"
end

-- nih-plug's state is a little-endian u64 length and then JSON, whatever the format wraps around it.
-- REAPER puts a VST3's in front of the controller's: u32 size, u32 flags, then those bytes.
local function state_json(bytes)
  local at = bytes:find('{"version"', 1, true)
  if not at then return nil end
  local len = string.unpack("<I8", bytes, at - 8)
  return at, len, bytes:sub(at, at + len - 1)
end

-- REAPER has no chunk call for an AU: its state is an XML property list in the track's, base64 twice.
local function au_state(tr)
  local _, whole = reaper.GetTrackStateChunk(tr, "", false)
  local block = whole:match("<AU[^\n]*\n(.-)\n>")
  if not block then return "" end
  local parts = {}
  for segment in (block:gsub("%s", "")):gmatch("[%w+/]+=*") do parts[#parts + 1] = b64_decode(segment) end
  local data = table.concat(parts):match("<key>data</key>%s*<data>(.-)</data>")
  if not data then return "" end
  local at, _, text = state_json(b64_decode(data))
  return at and text or ""
end

local function replace_between(s, from, to, new)
  local a = s:find(from, 1, true)
  assert(a, "no " .. from)
  local b = s:find(to, a + #from, true)
  assert(b, "no " .. to)
  return s:sub(1, a - 1) .. new .. s:sub(b)
end

-- The settings are a JSON string inside the state's JSON, hence the escaped quotes.
local function patch_settings(tr, fx, format, patch)
  local ok, chunk = reaper.TrackFX_GetNamedConfigParm(tr, fx, chunk_name(format))
  assert(ok and #chunk > 0, "can't read " .. chunk_name(format))
  local bytes = b64_decode(chunk)
  local at, len, text = state_json(bytes)
  assert(at, "no state in the chunk")
  text = patch(text)
  local before = bytes:sub(1, at - 9)
  if format == "VST3" then before = string.pack("<I4", #text + 8) .. before:sub(5) end
  bytes = before .. string.pack("<I8", #text) .. text .. bytes:sub(at + len)
  assert(reaper.TrackFX_SetNamedConfigParm(tr, fx, chunk_name(format), b64_encode(bytes)), "can't write " .. chunk_name(format))
end

local function setting(key, new)
  return function(text)
    return (text:gsub('\\"' .. key .. '\\":[^,}]*', function() return '\\"' .. key .. '\\":' .. new end, 1))
  end
end

local function add_instrument(format)
  reaper.InsertTrackAtIndex(0, false)
  local tr = reaper.GetTrack(0, 0)
  return tr, reaper.TrackFX_AddByName(tr, format .. ":CHIMERA", false, -1)
end

local function add_notes(tr, notes, seconds)
  local item = reaper.CreateNewMIDIItemInProj(tr, 0, seconds, false)
  local take = reaper.GetActiveTake(item)
  for _, n in ipairs(notes) do
    local a = reaper.MIDI_GetPPQPosFromProjTime(take, n[1])
    local b = reaper.MIDI_GetPPQPosFromProjTime(take, n[1] + n[3])
    reaper.MIDI_InsertNote(take, false, false, a, b, 0, n[2], 100, true)
  end
  reaper.MIDI_Sort(take)
  return item
end

local function render(name, seconds)
  reaper.GetSetProjectInfo_String(0, "RENDER_FILE", OUT, true)
  reaper.GetSetProjectInfo_String(0, "RENDER_PATTERN", name, true)
  reaper.GetSetProjectInfo_String(0, "RENDER_FORMAT", "evaw", true)
  reaper.GetSetProjectInfo(0, "RENDER_SETTINGS", 0, true)
  reaper.GetSetProjectInfo(0, "RENDER_BOUNDSFLAG", 0, true)
  reaper.GetSetProjectInfo(0, "RENDER_STARTPOS", 0, true)
  reaper.GetSetProjectInfo(0, "RENDER_ENDPOS", seconds, true)
  reaper.GetSetProjectInfo(0, "RENDER_CHANNELS", 2, true)
  reaper.GetSetProjectInfo(0, "RENDER_SRATE", FS, true)
  reaper.Main_OnCommand(42230, 0) -- render with these settings, and close the dialog
end

local function pdc(tr, fx)
  local _, value = reaper.TrackFX_GetNamedConfigParm(tr, fx, "pdc")
  return tonumber(value) or -1
end

-- Waits let the editor load and draw, so this runs as a coroutine, resumed from the host's loop.
local function sleep(seconds)
  local until_ = reaper.time_precise() + seconds
  while reaper.time_precise() < until_ do coroutine.yield() end
end

-- Hand over to the script that started REAPER, which removes the marker when it's done.
local function ask(step)
  touch("at-" .. step)
  local go = OUT .. "/go-" .. step
  local deadline = reaper.time_precise() + 120
  while reaper.time_precise() < deadline do
    local f = io.open(go)
    if f then
      f:close()
      return true
    end
    coroutine.yield()
  end
  return false
end

-- Each case: a fresh instance with these settings and notes, rendered. check.ts knows what each
-- should sound like.
local KICK = '{\\"id\\":\\"B\\"}'
local REST = "null"
local function steps(on)
  local s = {}
  for i = 0, 15 do s[#s + 1] = (i % 4 == 0 and on) and KICK or REST end
  return "[" .. table.concat(s, ",") .. "]"
end
local function steps_patch(on)
  return function(text)
    return replace_between(text, '\\"steps\\":[', ',\\"bpm\\"', '\\"steps\\":' .. steps(on))
  end
end
local function speech_patch(text)
  return replace_between(text, '\\"speech\\":{', '},\\"beat\\"', '\\"speech\\":{\\"text\\":\\"one two three\\\\nfour five six\\",\\"ipa\\":false,\\"rate\\":1.0,\\"start_line\\":0')
end

local CASES = {
  { name = "sing", seconds = 7, notes = { { 0.5, 48, 1 }, { 2.0, 52, 1 }, { 3.5, 55, 1 }, { 5.0, 60, 1 } } },
  { name = "beatbox", seconds = 8.5, patches = { setting("mode", '\\"beatbox\\"'), steps_patch(true) }, notes = {} },
  { name = "pads", seconds = 5, patches = { setting("mode", '\\"beatbox\\"'), steps_patch(false) }, notes = { { 1, 36, 0.1 }, { 2, 36, 0.1 }, { 3, 36, 0.1 }, { 4, 36, 0.1 } } },
  { name = "speak", seconds = 7, patches = { setting("mode", '\\"speak\\"'), speech_patch }, notes = { { 0.5, 60, 0.2 }, { 3.5, 60, 0.2 } } },
}

local function run_case(format, case)
  local tr, fx = add_instrument(format)
  if fx < 0 then
    record({ format = format, case = case.name, error = "the host couldn't load it" })
    return
  end
  local _, name = reaper.TrackFX_GetFXName(tr, fx, "")
  -- REAPER has no call for an AU's state, so those settings can only be reached from its editor
  if format == "AU" and case.patches then
    record({ format = format, case = case.name, skipped = "REAPER can't set an AU's state from a script" })
    reaper.DeleteTrack(tr)
    return
  end
  local good, err = pcall(function()
    for _, patch in ipairs(case.patches or {}) do patch_settings(tr, fx, format, patch) end
    -- a new latency restarts a VST3, a moment later: not while it renders
    if case.patches then sleep(1.5) end
    add_notes(tr, case.notes, case.seconds)
    render(format .. "-" .. case.name, case.seconds)
  end)
  record({ format = format, case = case.name, fx_name = name, pdc = pdc(tr, fx), error = good and "" or tostring(err) })
  reaper.DeleteTrack(tr)
end

local function state_of(tr, fx, format)
  if format == "AU" then return au_state(tr) end
  local ok, chunk = reaper.TrackFX_GetNamedConfigParm(tr, fx, chunk_name(format))
  if not ok then return "" end
  local at, _, text = state_json(b64_decode(chunk))
  return at and text or ""
end

local function window_of(state)
  return state:match('\"window\":\"%[(%d+,%d+)%]') or "?"
end

-- What someone does with it: open the editor, play, click a mode, drag the grip, and bounce.
local function editor(format)
  local tr, fx = add_instrument(format)
  if fx < 0 then return end
  local _, name = reaper.TrackFX_GetFXName(tr, fx, "")
  reaper.TrackFX_Show(tr, fx, 3) -- a floating window
  sleep(5)
  local opened = window_of(state_of(tr, fx, format))

  -- through the sound device, for the page to show live
  local item = add_notes(tr, { { 0.2, 60, 0.8 }, { 1.2, 64, 0.8 }, { 2.2, 67, 0.8 } }, 4)
  reaper.OnPlayButton()
  sleep(2.5)
  ask("shot-playing-" .. format)
  reaper.OnStopButton()
  sleep(1)
  reaper.DeleteTrackMediaItem(tr, item)

  local answered = ask("editor-" .. format)
  sleep(2) -- the grip's size settles once it's let go
  local state = state_of(tr, fx, format)
  record({ format = format, case = "editor", answered = answered, pdc = pdc(tr, fx), mode = state:match('\\"mode\\":\\"(%a+)\\"') or "?", opened = opened, window = window_of(state) })

  -- the mode the click chose, with its example pattern, and the editor open
  local good, err = pcall(render, format .. "-clicked", 8.5)
  record({ format = format, case = "clicked", fx_name = name, pdc = pdc(tr, fx), error = good and "" or tostring(err) })

  reaper.TrackFX_Show(tr, fx, 2)
  sleep(1)
  reaper.DeleteTrack(tr)
end

local function main()
  -- not under a dialog of REAPER's: rendering does nothing there
  ask("ready")
  reaper.GetSetProjectInfo(0, "PROJECT_SRATE", FS, true)
  reaper.GetSetProjectInfo(0, "PROJECT_SRATE_USE", 1, true)
  reaper.SetCurrentBPM(0, BPM, false)
  record({ reaper = reaper.GetAppVersion(), os = reaper.GetOS() })
  for _, format in ipairs(FORMATS) do
    for _, case in ipairs(CASES) do run_case(format, case) end
  end
  if os.getenv("CHIMERA_TEST_EDITOR") == "1" then
    for _, format in ipairs(FORMATS) do editor(format) end
  end
end

local co = coroutine.create(main)
local function tick()
  local good, err = coroutine.resume(co)
  if not good then record({ error = debug.traceback(co, tostring(err)) }) end
  if coroutine.status(co) == "dead" then
    results:close()
    touch("finished")
  else
    reaper.defer(tick)
  end
end
tick()
