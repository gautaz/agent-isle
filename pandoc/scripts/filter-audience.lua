-- Pandoc Lua filter for audience-based content filtering
-- Used to generate README.md, AGENT.md, and CONTRIBUTING.md from a single source
--
-- Usage:
--   pandoc --metadata=audience:readme source.mkd -o README.md
--   pandoc --metadata=audience:agent source.mkd -o AGENT.md
--   pandoc --metadata=audience:contributing source.mkd -o CONTRIBUTING.md
--
-- Content in ::: {.readme} blocks appears only in README.md.
-- Content in ::: {.agent} blocks appears only in AGENT.md.
-- Content in ::: {.contributing} blocks appears only in CONTRIBUTING.md.
-- Content without audience classes appears in all outputs.

-- Track the target audience from metadata
local audience = nil

function Meta(meta)
  if meta.audience then
    audience = pandoc.utils.stringify(meta.audience)
  end
end

-- Filter Div elements based on audience class
function Div(elem)
  if not audience then
    return nil
  end

  -- Collect audience-specific classes on this div
  local audienceClasses = {}
  for _, class in ipairs(elem.classes) do
    if class == "readme" or class == "agent" or class == "contributing" then
      table.insert(audienceClasses, class)
    end
  end

  -- If div has no audience classes, keep it in all outputs
  if #audienceClasses == 0 then
    return nil
  end

  -- Keep content only if target audience matches one of the div's classes
  for _, class in ipairs(audienceClasses) do
    if audience == class then
      return elem.content
    end
  end

  -- Target audience doesn't match — remove content
  return {}
end

return {
  { Meta = Meta },
  { Div = Div }
}
