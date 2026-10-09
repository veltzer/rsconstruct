-- Generates src/engines/luacheck/tables.json for iluacheck from luacheck's own sources:
-- the built-in standards (luacheck.builtin_standards, as luacheck builds them
-- when it runs on Lua 5.1, which decides what `_G` means) and the Unicode
-- printability boundaries.
--
-- Usage: lua5.1 scripts/gen-iluacheck-tables.lua LUACHECK_LUA_DIR > src/engines/luacheck/tables.json
-- where LUACHECK_LUA_DIR holds luacheck/ (the apt package: /usr/share/lua/5.1;
-- a checkout: its src/ directory).
--
-- A Lua table is written as {"a": [array part], "h": [[key, value], ...]}
-- with the hash part sorted by key, so the output is deterministic. Strings,
-- booleans and numbers are written as JSON scalars.

local dir = assert(arg[1], "usage: gen-iluacheck-tables.lua LUACHECK_LUA_DIR")
package.path = dir .. "/?.lua;" .. dir .. "/?/init.lua;" .. package.path

local builtin_standards = require "luacheck.builtin_standards"
local boundaries = require "luacheck.unicode_printability_boundaries"

local function json_string(s)
   return '"' .. s:gsub('[%c"\\\128-\255]', function(c)
      return ("\\u%04x"):format(c:byte())
   end) .. '"'
end

local encode

local function encode_table(t)
   local array = {}

   for _, v in ipairs(t) do
      table.insert(array, encode(v))
   end

   local keys = {}

   for k in pairs(t) do
      if not (type(k) == "number" and k >= 1 and k <= #t and k == math.floor(k)) then
         assert(type(k) == "string", "unexpected key type " .. type(k))
         table.insert(keys, k)
      end
   end

   table.sort(keys)
   local hash = {}

   for _, k in ipairs(keys) do
      table.insert(hash, "[" .. json_string(k) .. "," .. encode(t[k]) .. "]")
   end

   return '{"a":[' .. table.concat(array, ",") .. '],"h":[' .. table.concat(hash, ",") .. "]}"
end

function encode(v)
   if type(v) == "table" then
      return encode_table(v)
   elseif type(v) == "string" then
      return json_string(v)
   elseif type(v) == "boolean" then
      return tostring(v)
   elseif type(v) == "number" then
      return ("%.17g"):format(v)
   else
      error("unexpected value type " .. type(v))
   end
end

local names = {}

for name in pairs(builtin_standards) do
   table.insert(names, name)
end

table.sort(names)
local stds = {}

for _, name in ipairs(names) do
   table.insert(stds, "[" .. json_string(name) .. "," .. encode(builtin_standards[name]) .. "]")
end

local numbers = {}

for _, n in ipairs(boundaries) do
   table.insert(numbers, ("%d"):format(n))
end

io.write('{"standards":[', table.concat(stds, ",\n"), '],\n"printability_boundaries":[',
   table.concat(numbers, ","), "]}\n")
