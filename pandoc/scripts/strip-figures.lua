-- Pandoc Lua filter to strip Figure wrappers, rendering images as plain <img>
--
-- pandoc 3.x parses standalone ![alt](url) as a Figure block, and the GFM
-- writer wraps it in <figure><figcaption>. This filter converts Figure back
-- to a Plain containing just the Image inline, producing <img> directly.

function Figure(fig)
  for _, block in ipairs(fig.content) do
    if block.t == "Plain" or block.t == "Para" then
      for _, inline in ipairs(block.content) do
        if inline.t == "Image" then
          return pandoc.Plain({inline})
        end
      end
    end
  end
  return nil
end

return {
  { Figure = Figure }
}
