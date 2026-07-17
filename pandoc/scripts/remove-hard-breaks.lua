-- Pandoc Lua filter to remove hard line break markers
--
-- Converts hard line breaks (LineBreak elements) to soft line breaks.
-- This removes the trailing `\` from markdown output while preserving
-- the newline in the source.
--
-- Usage:
--   pandoc source.mkd --lua-filter=remove-hard-breaks.lua -o output.md
--
-- Note: This filter operates on the AST before output generation.
-- LineBreak elements are replaced with SoftBreak elements, which
-- render as plain newlines in markdown output.

function LineBreak(linebreak)
    return pandoc.SoftBreak()
end

return {
    { LineBreak = LineBreak }
}
