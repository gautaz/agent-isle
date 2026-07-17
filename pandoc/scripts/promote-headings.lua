-- Pandoc Lua filter to promote headings by one level
-- Preserves the first h1 as the document title
-- All subsequent headings are promoted by one level (h1 → h2, h2 → h3, etc.)
--
-- Usage:
--   pandoc --lua-filter=promote-headings.lua ...

local first_h1_seen = false

function Header(elem)
  if elem.level == 1 and not first_h1_seen then
    -- Preserve the first h1 (document title)
    first_h1_seen = true
    return nil
  end

  -- Promote heading by one level (cap at h6)
  if elem.level < 6 then
    elem.level = elem.level + 1
  end

  return elem
end

return {
  { Header = Header }
}
