--- ghost.template — simple mustache-style template rendering.
---
--- Usage:
---   local template = require("ghost.template")
---   template.render("Hello {{name}}, today is {{date}}", { name = "Ghost", date = "2026-03-01" })
---   --> "Hello Ghost, today is 2026-03-01"

local M = {}

--- Render a template string by replacing `{{key}}` placeholders with values
--- from the provided table. Supports optional whitespace inside braces:
--- `{{ key }}` and `{{key}}` both work.
---
--- Missing keys are left as-is (no error).
---@param text string  The template string
---@param vars table<string, string|number>  Key-value pairs to substitute
---@return string
function M.render(text, vars)
    if not vars then return text end
    return (text:gsub("{{%s*(.-)%s*}}", function(key)
        local val = vars[key]
        if val ~= nil then
            return tostring(val)
        end
        -- Leave unknown placeholders untouched
        return "{{" .. key .. "}}"
    end))
end

return M
