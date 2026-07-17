-- Pandoc Lua filter for checking documentation rules
--
-- Extensible rule system: add new rules by creating a check function
-- and registering it in the RULES table below.
--
-- Usage:
--   pandoc source.mkd --lua-filter=check-rules.lua -o /dev/null
--
-- The filter prints warnings to stderr and returns the AST unchanged.
-- The calling script should check stderr for output and exit non-zero
-- if any warnings are found.

-- Rule registry
-- Each rule is a function that receives a pandoc element and returns
-- a warning string (or nil if no violation).
local RULES = {}

-- Rule: one sentence per line
-- Checks that no line contains multiple sentences.
-- A paragraph CAN have multiple sentences (on separate lines),
-- but each sentence MUST be on its own line.
function RULES.one_sentence_per_line(elem)
    local text = pandoc.utils.stringify(elem)

    -- Collect byte positions of LineBreak/SoftBreak in the stringified text.
    -- stringify collapses LineBreak to " ", so we reconstruct line boundaries
    -- from the inline elements to split correctly.
    local break_positions = {}
    local pos = 0
    local prev_was_break = false

    for _, inline in ipairs(elem.content) do
        if inline.t == "Str" then
            pos = pos + #inline.text
            prev_was_break = false
        elseif inline.t == "Code" then
            pos = pos + #inline.text
            prev_was_break = false
        elseif inline.t == "Strong" or inline.t == "Emph" then
            for _, child in ipairs(inline.content) do
                if child.t == "Str" then
                    pos = pos + #child.text
                elseif child.t == "Code" then
                    pos = pos + #child.text
                elseif child.t == "Space" then
                    pos = pos + 1
                end
            end
            prev_was_break = false
        elseif inline.t == "Space" then
            if not prev_was_break then
                pos = pos + 1
            end
            prev_was_break = false
        elseif inline.t == "LineBreak" or inline.t == "SoftBreak" then
            pos = pos + 1  -- stringify replaces LineBreak with " "
            table.insert(break_positions, pos)
            prev_was_break = true
        end
    end

    -- Split text into lines at break positions
    local lines = {}
    local start = 1
    for _, bp in ipairs(break_positions) do
        table.insert(lines, text:sub(start, bp - 1))
        start = bp + 1
    end
    table.insert(lines, text:sub(start))

    -- Count sentences per line
    for _, line in ipairs(lines) do
        local sentence_count = 0
        for _ in line:gmatch("[%.%?!]+%s+[A-Z]") do
            sentence_count = sentence_count + 1
        end
        sentence_count = sentence_count + 1

        if sentence_count > 1 then
            return string.format(
                "one sentence per line: %d sentences on 1 line:\n  %s",
                sentence_count,
                line:sub(1, 80) .. (line:len() > 80 and "..." or "")
            )
        end
    end

    return nil
end

-- Checker for block elements (Para, Plain)
local function check_block(elem, rule_name)
    local warnings = {}

    for name, rule_fn in pairs(RULES) do
        local warning = rule_fn(elem)
        if warning then
            table.insert(warnings, string.format("[%s] %s", name, warning))
        end
    end

    if #warnings > 0 then
        for _, w in ipairs(warnings) do
            io.stderr:write("Warning: " .. w .. "\n")
        end
    end

    return nil  -- Don't modify the AST
end

local function check_para(elem)
    return check_block(elem, "para")
end

local function check_plain(elem)
    return check_block(elem, "plain")
end

return {
    { Para = check_para },
    { Plain = check_plain }
}
