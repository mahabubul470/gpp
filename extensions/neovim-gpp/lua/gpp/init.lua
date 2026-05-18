-- neovim-gpp — Lua plugin exposing gpp via Telescope pickers and inline
-- virtual text. Falls back to a plain vim.ui.select list if Telescope is
-- not installed. The `gpp` CLI is the single source of truth.

local M = {}

local function gpp(args)
  local out = vim.fn.systemlist({ "gpp", unpack(args) })
  if vim.v.shell_error ~= 0 then
    vim.notify("gpp: " .. table.concat(out, "\n"), vim.log.levels.ERROR)
    return {}
  end
  return out
end

local function pick(title, lines, on_choice)
  local ok, pickers = pcall(require, "telescope.pickers")
  if ok then
    local finders = require("telescope.finders")
    local conf = require("telescope.config").values
    local actions = require("telescope.actions")
    local state = require("telescope.actions.state")
    pickers.new({}, {
      prompt_title = title,
      finder = finders.new_table({ results = lines }),
      sorter = conf.generic_sorter({}),
      attach_mappings = function(buf)
        actions.select_default:replace(function()
          actions.close(buf)
          if on_choice then on_choice(state.get_selected_entry()[1]) end
        end)
        return true
      end,
    }):find()
  else
    vim.ui.select(lines, { prompt = title }, on_choice)
  end
end

function M.timeline() pick("gpp timeline", gpp({ "timeline", "-n", "100" })) end
function M.log() pick("gpp log", gpp({ "log", "--oneline" })) end

function M.graphex_query()
  vim.ui.input({ prompt = "graphex query: " }, function(q)
    if q and #q > 0 then pick("graphex: " .. q, gpp({ "graphex", "query", q })) end
  end)
end

function M.review()
  pick("gpp reviews", gpp({ "review", "list" }), function(line)
    local cs = line:match("^(%S+)")
    if cs then vim.cmd("new | r !gpp review show " .. cs) end
  end)
end

-- Inline virtual text: annotate the current line with the latest timeline
-- author for this file (best-effort).
function M.annotate()
  local ns = vim.api.nvim_create_namespace("gpp")
  vim.api.nvim_buf_clear_namespace(0, ns, 0, -1)
  local file = vim.fn.expand("%")
  local out = gpp({ "timeline", "--file", file, "-n", "1" })
  if out[1] then
    vim.api.nvim_buf_set_extmark(0, ns, 0, 0, {
      virt_text = { { " gpp: " .. out[1], "Comment" } },
      virt_text_pos = "eol",
    })
  end
end

function M.setup()
  vim.api.nvim_create_user_command("GppTimeline", M.timeline, {})
  vim.api.nvim_create_user_command("GppLog", M.log, {})
  vim.api.nvim_create_user_command("GppGraphex", M.graphex_query, {})
  vim.api.nvim_create_user_command("GppReview", M.review, {})
  vim.api.nvim_create_user_command("GppAnnotate", M.annotate, {})
end

return M
