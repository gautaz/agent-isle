-- Pandoc Lua filter to include files via HTML comment markers or code block attributes
--
-- Markers in source:
--   <!-- include:FILE -->           (whole file)
--   <!-- include:FILE:SECTION -->  (section between @section markers)
--
-- Code block syntax:
--   ```{.LANG include="FILE" section="SECTION"}
--   ```
--
-- Supports arbitrary nesting depth via recursive processing.

local include_dir = "."

function Meta(meta)
  if meta.include_dir then
    include_dir = pandoc.utils.stringify(meta.include_dir)
  end
end

-- Read file and extract content (whole file or section)
function read_include(file, section)
  -- Resolve path
  local path = file
  if not path:match("^/") then
    path = include_dir .. "/" .. file
  end

  -- Read file
  local f = io.open(path, "r")
  if not f then
    io.stderr:write("include.lua: cannot open " .. path .. "\n")
    return nil
  end

  local lines = {}
  for line in f:lines() do
    table.insert(lines, line)
  end
  f:close()

  local extracted = {}

  if section then
    -- Find section start and end markers
    -- Markers can be in any comment syntax: # @section:name, // @section:name, etc.
    -- Section ends with @end:name or next @section:
    local sectionStart = nil
    local sectionEnd = nil
    local marker_pattern = "@section:" .. section
    local end_pattern = "@end:" .. section

    for i, line in ipairs(lines) do
      if line:find(marker_pattern, 1, true) then
        sectionStart = i + 1
      elseif sectionStart and (line:find(end_pattern, 1, true) or line:find("@section:", 1, true)) then
        sectionEnd = i - 1
        break
      end
    end

    if not sectionStart then
      io.stderr:write("include.lua: section '" .. section .. "' not found in " .. path .. "\n")
      return nil
    end

    if not sectionEnd then
      sectionEnd = #lines
    end

    for i = sectionStart, sectionEnd do
      table.insert(extracted, lines[i])
    end
  else
    -- Whole-file include
    for _, line in ipairs(lines) do
      table.insert(extracted, line)
    end
  end

  while #extracted > 0 and extracted[#extracted]:match("^%s*$") do
    table.remove(extracted)
  end

  if #extracted == 0 then
    return nil
  end

  return table.concat(extracted, "\n")
end

-- Process a single include marker, returning blocks or nil.
function process_include(text)
  local prefix = "<!-- include:"
  if text:sub(1, #prefix) ~= prefix then
    return nil
  end

  local rest = text:sub(#prefix + 1)
  local suffix = " -->"
  if rest:sub(-#suffix) ~= suffix then
    return nil
  end

  rest = rest:sub(1, -#suffix - 1)

  local colon = rest:find(":")
  local file, section
  if colon then
    file = rest:sub(1, colon - 1)
    section = rest:sub(colon + 1)
  else
    file = rest
    section = nil
  end

  local content = read_include(file, section)
  if not content then
    return nil
  end

  if section then
    -- Section-based includes are raw content (e.g., YAML), wrap in code block
    return pandoc.CodeBlock(content, {class = "yaml"})
  else
    -- Whole-file includes are markdown, parse and process recursively
    local doc = pandoc.read(content, "markdown")
    return process_blocks(doc.blocks)
  end
end

-- Recursively process blocks, resolving any include markers.
function process_blocks(blocks)
  local result = {}
  for _, block in ipairs(blocks) do
    if block.t == "RawBlock" and block.format == "html" then
      local text = block.text:match("^%s*(.-)%s*$")
      local included = process_include(text)
      if included then
        if included.t then
          -- Single block (e.g., CodeBlock from section include)
          table.insert(result, included)
        else
          -- List of blocks (from whole-file include, already recursively processed)
          for _, b in ipairs(included) do
            table.insert(result, b)
          end
        end
      else
        table.insert(result, block)
      end
    elseif block.t == "Div" then
      -- Recursively process content inside fenced divs
      block.content = process_blocks(block.content)
      table.insert(result, block)
    else
      table.insert(result, block)
    end
  end
  return result
end

function RawBlock(elem)
  if elem.format ~= "html" then
    return nil
  end

  local text = elem.text:match("^%s*(.-)%s*$")
  local included = process_include(text)
  return included
end

-- Handle code blocks with include attribute
-- Syntax: ```{.lang include="file" section="name"} ```
function CodeBlock(elem)
  local include_attr = elem.attr.attributes["include"]
  if not include_attr then
    return nil
  end

  local file = include_attr
  local section = elem.attr.attributes["section"]
  local lang = elem.classes[1] or "text"

  local content = read_include(file, section)
  if not content then
    return nil
  end

  return pandoc.CodeBlock(content, {class = lang})
end

return {
  { Meta = Meta },
  { RawBlock = RawBlock },
  { CodeBlock = CodeBlock }
}
