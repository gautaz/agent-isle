-- Pandoc Lua filter to fix GitHub callout syntax in GFM output
--
-- pandoc's GFM writer escapes brackets in blockquote content,
-- producing \[!NOTE\] instead of [!NOTE]. This filter restores
-- the correct syntax for GitHub callouts.
--
-- Usage:
--   pandoc --lua-filter=fix-callouts.lua --to gfm ...

local CALLOUT_TYPES = { "NOTE", "WARNING", "TIP", "IMPORTANT", "CAUTION" }

local function render_inlines(inlines)
    local lines = { "" }
    for _, inline in ipairs(inlines) do
        if inline.t == "LineBreak" or inline.t == "SoftBreak" then
            table.insert(lines, "")
        elseif inline.t == "Str" then
            lines[#lines] = lines[#lines] .. inline.text
        elseif inline.t == "Space" then
            lines[#lines] = lines[#lines] .. " "
        elseif inline.t == "Code" then
            lines[#lines] = lines[#lines] .. "`" .. inline.text .. "`"
        elseif inline.t == "Emph" then
            lines[#lines] = lines[#lines] .. "*" .. pandoc.utils.stringify(inline) .. "*"
        elseif inline.t == "Strong" then
            lines[#lines] = lines[#lines] .. "**" .. pandoc.utils.stringify(inline) .. "**"
        else
            lines[#lines] = lines[#lines] .. pandoc.utils.stringify(inline)
        end
    end
    return lines
end

function BlockQuote(quote)
    -- Find callout marker in first Para/Plain
    for _, block in ipairs(quote.content) do
        if block.t == "Para" or block.t == "Plain" then
            local first = block.content[1]
            if first and first.t == "Str" then
                for _, callout in ipairs(CALLOUT_TYPES) do
                    local marker = "[!" .. callout .. "]"
                    if first.text == marker then
                        -- Collect all lines from all blocks
                        local all_lines = {}

                        -- Remaining inlines from first block (skip marker)
                        local rest_inlines = { table.unpack(block.content, 2) }
                        if #rest_inlines > 0 then
                            all_lines = render_inlines(rest_inlines)
                        end

                        -- Subsequent blocks
                        local found_first = false
                        for _, b in ipairs(quote.content) do
                            if b == block then
                                found_first = true
                            elseif found_first and (b.t == "Para" or b.t == "Plain") then
                                local block_lines = render_inlines(b.content)
                                for _, line in ipairs(block_lines) do
                                    table.insert(all_lines, line)
                                end
                            end
                        end

                        -- Build raw blockquote
                        local raw = "> [!" .. callout .. "]\n"
                        for _, line in ipairs(all_lines) do
                            if line ~= "" then
                                raw = raw .. "> " .. line .. "\n"
                            end
                        end
                        return pandoc.RawBlock("markdown", raw)
                    end
                end
            end
        end
    end
    return nil
end

return {
  { BlockQuote = BlockQuote }
}
