-- A world profile (spec Tooling §16.1.4, decision T213), applied before the
-- addon loads: the character, game data, trained spells, money, stats, gear, bags, action
-- bars, chat channels, macros, collections, place, auras, the units
-- around, CVars, bindings and saved variables.
local mock, profile = ...
local function each(t, fn)
    if type(t) == "table" then
        for k, v in pairs(t) do
            fn(k, v)
        end
    end
end
local player = mock.player
if type(profile.character) == "table" then
    mock.setPlayer(profile.character)
end
each(profile.items, function(id, data)
    local opts = { cached = data.cached ~= false, delay = data.delay, missing = data.missing }
    mock.defineItem(tonumber(id), data, opts)
end)
each(profile.spells, function(id, data)
    mock.defineSpell(tonumber(id), data)
end)
if type(profile.trained) == "table" then
    local ids = {}
    each(profile.trained, function(_, id)
        ids[#ids + 1] = tonumber(id)
    end)
    player:learn(ids, { quiet = true })
end
if profile.money then
    player:setMoney(profile.money)
end
if type(profile.stats) == "table" then
    player:setStats(profile.stats)
end
each(profile.equipment, function(slot, item)
    if type(slot) == "string" and not tonumber(slot) then
        slot = GetInventorySlotInfo(slot)
    end
    player:equip(tonumber(slot), item)
end)
each(profile.bags, function(bag, contents)
    mock.setBag(tonumber(bag), contents)
end)
each(profile.actions, function(slot, action)
    mock.setAction(tonumber(slot), action)
end)
if type(profile.macros) == "table" then
    mock.setMacros(profile.macros)
end
-- The collections: the mount journal, the toy box, the wardrobe.
if type(profile.mounts) == "table" then
    mock.setMounts(profile.mounts)
end
if type(profile.toys) == "table" then
    mock.setToys(profile.toys)
end
if type(profile.appearances) == "table" then
    mock.setAppearances(profile.appearances)
end
if type(profile.channels) == "table" then
    mock.setChannels(profile.channels)
end
if type(profile.position) == "table" then
    mock.moveTo(profile.position)
end
if type(profile.time) == "string" then
    local hour, minute = profile.time:match("^(%d+):(%d+)$")
    if hour then
        mock.setTimeOfDay(tonumber(hour), tonumber(minute))
    end
end
if type(profile.weather) == "table" and type(profile.weather.kind) == "string" then
    mock.setWeather(profile.weather.kind, profile.weather.intensity)
end
each(profile.auras, function(_, aura)
    player:applyAura(aura)
end)
each(profile.renown, function(faction, level)
    mock.setRenown(tonumber(faction), level)
end)
each(profile.units, function(_, data)
    -- Another player of the world, not in the group: online, and accepting
    -- the player's invitations, as a friend would (decision T363).
    if data.player and not data.group and data.acceptsInvites == nil then
        data.acceptsInvites = true
    end
    local unit = mock.spawnUnit(data)
    if data.target then
        player:target(unit)
    end
    if data.focus then
        player:focus(unit)
    end
    if data.group then
        mock.addToGroup(unit, data.group == "raid")
    end
    if data.nameplate then
        unit:showNameplate({ entering = true })
    end
    -- What it is to the player (decision T530): a vendor, a class trainer,
    -- gossip, a service.
    if type(data.vendor) == "table" then
        local v = data.vendor
        mock.defineVendor(unit, { name = data.name, items = v.items, canRepair = v.repairs })
    end
    if type(data.trainer) == "table" then
        mock.defineTrainer(unit, data.trainer)
    end
    local gossip = data.gossip
    if data.service == "flightmaster" and type(gossip) ~= "table" then
        gossip = { options = { { name = "Show me where I can fly.", opens = "taxi" } } }
    end
    if type(gossip) == "table" then
        each(gossip.options, function(_, o)
            if o.opens == "vendor" and type(data.vendor) == "table" then
                o.opens = "merchant"
                o.merchant = { name = data.name, items = data.vendor.items, canRepair = data.vendor.repairs }
            end
        end)
        mock.defineGossip(unit, gossip)
    end
    if data.service == "flightmaster" then
        mock.defineTaxi(unit, data.taxi or {})
    elseif data.service == "banker" then
        mock.defineBank(unit, data.bank or {})
    elseif data.service == "mailbox" then
        mock.defineMailbox(unit, data.mailbox or {})
    elseif data.service == "auctioneer" then
        mock.defineAuctionHouse(unit, data.auctionHouse or {})
    end
    if type(data.threat) == "number" then
        unit:setThreat(data.threat)
    end
    each(data.auras, function(_, aura)
        unit:applyAura(aura)
    end)
end)
each(profile.cvars, function(name, value)
    C_CVar.SetCVar(name, tostring(value))
end)
each(profile.bindings, function(key, action)
    SetBinding(key, action)
end)
each(profile.savedVariables, function(name, value)
    _G[name] = value
end)
