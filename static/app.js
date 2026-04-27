"use strict";

/* =========================================================================
 * 7DTD Server Manager — front-end controller
 *
 * No build step, no framework. Vanilla JS talking to the Rust backend over
 * /api/*. Polls /api/status and /api/logs while the page is open.
 * ========================================================================= */

// ── Settings categorization ────────────────────────────────────────────
//
// We don't hardcode the property list — we render whatever the server's
// XML actually contains, so this UI keeps working as the game adds/removes
// settings between versions. We DO classify known names into groups, and
// hint at the input type so checkboxes/numbers/enums render correctly.
// Anything unknown lands in "Misc / Advanced" as a text field.
//
// Reference: 7DTD V2.6 (April 2026) default serverconfig.xml.

const KNOWN_GROUPS = [
    {
        name: "Server Identity",
        match: ["ServerName", "ServerDescription", "ServerWebsiteURL",
                "ServerLoginConfirmationText", "Region", "Language",
                "ServerVisibility"],
    },
    {
        name: "Networking",
        match: ["ServerPort", "ServerDisabledNetworkProtocols",
                "ServerMaxWorldTransferSpeedKiBs", "ServerPassword"],
    },
    {
        name: "Slots",
        match: ["ServerMaxPlayerCount", "ServerReservedSlots",
                "ServerReservedSlotsPermission", "ServerAdminSlots",
                "ServerAdminSlotsPermission"],
    },
    {
        name: "Admin Interfaces",
        match: ["WebDashboardEnabled", "WebDashboardPort", "WebDashboardUrl",
                "EnableMapRendering",
                "TelnetEnabled", "TelnetPort", "TelnetPassword",
                "TelnetFailedLoginLimit", "TelnetFailedLoginsBlocktime",
                "TerminalWindowEnabled"],
    },
    {
        name: "Folders & Files",
        match: ["AdminFileName", "UserDataFolder", "SaveGameFolder"],
    },
    {
        name: "Anti-cheat & Logging",
        match: ["EACEnabled", "HideCommandExecutionLog",
                "MaxUncoveredMapChunksPerPlayer", "PersistentPlayerProfiles"],
    },
    {
        name: "World",
        match: ["GameWorld", "WorldGenSeed", "WorldGenSize",
                "GameName", "GameMode"],
    },
    {
        name: "Difficulty",
        match: ["GameDifficulty", "BlockDamagePlayer", "BlockDamageAI",
                "BlockDamageAIBM", "XPMultiplier",
                "PlayerSafeZoneLevel", "PlayerSafeZoneHours"],
    },
    {
        name: "Day / Night",
        match: ["BuildCreate", "DayNightLength", "DayLightLength",
                "DeathPenalty", "DropOnDeath", "DropOnQuit",
                "BedrollDeadZoneSize", "BedrollExpiryTime",
                "CameraRestrictionMode"],
    },
    {
        name: "Performance",
        match: ["MaxSpawnedZombies", "MaxSpawnedAnimals",
                "ServerMaxAllowedViewDistance", "MaxQueuedMeshLayers"],
    },
    {
        name: "Zombies & Blood Moon",
        match: ["EnemySpawnMode", "EnemyDifficulty", "ZombieFeralSense",
                "ZombieMove", "ZombieMoveNight", "ZombieFeralMove",
                "ZombieBMMove",
                "BloodMoonFrequency", "BloodMoonRange",
                "BloodMoonWarning", "BloodMoonEnemyCount"],
    },
    {
        name: "Loot & Air Drops",
        match: ["LootAbundance", "LootRespawnDays", "JarRefund",
                "AirDropFrequency", "AirDropMarker"],
    },
    {
        name: "Multiplayer",
        match: ["PartySharedKillRange", "PlayerKillingMode"],
    },
    {
        name: "Land Claims",
        match: ["LandClaimCount", "LandClaimSize", "LandClaimDeadZone",
                "LandClaimExpiryTime", "LandClaimDecayMode",
                "LandClaimOnlineDurabilityModifier",
                "LandClaimOfflineDurabilityModifier",
                "LandClaimOfflineDelay"],
    },
    {
        name: "Dynamic Mesh",
        match: ["DynamicMeshEnabled", "DynamicMeshLandClaimOnly",
                "DynamicMeshLandClaimBuffer", "DynamicMeshMaxItemCache"],
    },
    {
        name: "Twitch Integration",
        match: ["TwitchServerPermission", "TwitchBloodMoonAllowed"],
    },
    {
        name: "World Persistence",
        match: ["MaxChunkAge", "SaveDataLimit"],
    },
];

const KNOWN_BOOLS = new Set([
    "WebDashboardEnabled", "EnableMapRendering",
    "TelnetEnabled", "TerminalWindowEnabled", "EACEnabled",
    "PersistentPlayerProfiles", "BuildCreate",
    "EnemySpawnMode", "AirDropMarker",
    "DynamicMeshEnabled", "DynamicMeshLandClaimOnly",
    "TwitchBloodMoonAllowed",
]);

const KNOWN_NUMS = new Set([
    "ServerPort", "ServerMaxWorldTransferSpeedKiBs",
    "ServerMaxPlayerCount", "ServerReservedSlots",
    "ServerReservedSlotsPermission", "ServerAdminSlots",
    "ServerAdminSlotsPermission",
    "WebDashboardPort", "TelnetPort",
    "TelnetFailedLoginLimit", "TelnetFailedLoginsBlocktime",
    "MaxUncoveredMapChunksPerPlayer",
    "WorldGenSize",
    "BlockDamagePlayer", "BlockDamageAI", "BlockDamageAIBM",
    "XPMultiplier", "PlayerSafeZoneLevel", "PlayerSafeZoneHours",
    "DayNightLength", "DayLightLength",
    "BedrollDeadZoneSize", "BedrollExpiryTime",
    "MaxSpawnedZombies", "MaxSpawnedAnimals",
    "ServerMaxAllowedViewDistance", "MaxQueuedMeshLayers",
    "BloodMoonFrequency", "BloodMoonRange",
    "BloodMoonWarning", "BloodMoonEnemyCount",
    "LootAbundance", "LootRespawnDays",
    "AirDropFrequency",
    "PartySharedKillRange",
    "LandClaimCount", "LandClaimSize", "LandClaimDeadZone",
    "LandClaimExpiryTime",
    "LandClaimOnlineDurabilityModifier",
    "LandClaimOfflineDurabilityModifier",
    "LandClaimOfflineDelay",
    "DynamicMeshLandClaimBuffer", "DynamicMeshMaxItemCache",
    "TwitchServerPermission",
    "MaxChunkAge", "SaveDataLimit",
]);

// Discrete dropdowns for properties with a known small enum.
const KNOWN_ENUMS = {
    ServerVisibility:    [["0","Hidden (LAN only)"],["1","Friends only"],["2","Public"]],
    Region:              [["NorthAmericaEast","NorthAmericaEast"],["NorthAmericaWest","NorthAmericaWest"],
                          ["CentralAmerica","CentralAmerica"],["SouthAmerica","SouthAmerica"],
                          ["Europe","Europe"],["Russia","Russia"],["Asia","Asia"],
                          ["MiddleEast","MiddleEast"],["Africa","Africa"],["Oceania","Oceania"]],
    GameMode:            [["GameModeSurvival","Survival"]],
    GameDifficulty:      [["0","Scavenger (0)"],["1","Adventurer (1)"],["2","Nomad (2)"],
                          ["3","Warrior (3)"],["4","Survivalist (4)"],["5","Insane (5)"]],
    EnemyDifficulty:     [["0","Normal"],["1","Feral"]],
    ZombieFeralSense:    [["0","Off"],["1","Day"],["2","Night"],["3","All"]],
    ZombiesRun:          [["0","Walk"],["1","Jog"],["2","Run"],["3","Sprint"],["4","Nightmare"]],
    ZombieMove:          [["0","Walk"],["1","Jog"],["2","Run"],["3","Sprint"],["4","Nightmare"]],
    ZombieMoveNight:     [["0","Walk"],["1","Jog"],["2","Run"],["3","Sprint"],["4","Nightmare"]],
    ZombieFeralMove:     [["0","Walk"],["1","Jog"],["2","Run"],["3","Sprint"],["4","Nightmare"]],
    ZombieBMMove:        [["0","Walk"],["1","Jog"],["2","Run"],["3","Sprint"],["4","Nightmare"]],
    DeathPenalty:        [["0","None"],["1","Classic XP penalty"],["2","Injured"],["3","Permanent death"]],
    DropOnDeath:         [["0","Nothing"],["1","Everything"],["2","Toolbelt only"],["3","Backpack only"],["4","Delete all"]],
    DropOnQuit:          [["0","Nothing"],["1","Everything"],["2","Toolbelt only"],["3","Backpack only"]],
    PlayerKillingMode:   [["0","No killing"],["1","Allies only"],["2","Strangers only"],["3","Everyone"]],
    LandClaimDecayMode:  [["0","Slow (linear)"],["1","Fast (exponential)"],["2","None (full protection)"]],
    HideCommandExecutionLog: [["0","Show everything"],["1","Hide from telnet/control panel"],
                              ["2","Also hide from remote game clients"],["3","Hide everything"]],
    CameraRestrictionMode: [["0","Free 1st/3rd person"],["1","Restricted to 1st person"],["2","Restricted to 3rd person"]],
    JarRefund:           [["0","0%"],["5","5%"],["10","10%"],["20","20%"],["30","30%"],["40","40%"],
                          ["50","50%"],["60","60%"],["70","70%"],["80","80%"],["90","90%"],["100","100%"]],
};

// ── Tiny DOM helpers ───────────────────────────────────────────────────
const $  = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => Array.from(root.querySelectorAll(s));
const el = (tag, attrs = {}, ...children) => {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) {
        if (k === "class") e.className = v;
        else if (k === "dataset") Object.assign(e.dataset, v);
        else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
        else if (v !== undefined && v !== null) e.setAttribute(k, v);
    }
    for (const c of children) {
        if (c == null) continue;
        e.append(c.nodeType ? c : document.createTextNode(c));
    }
    return e;
};

// ── API client ─────────────────────────────────────────────────────────
const api = {
    async get(path)            { return req("GET", path); },
    async post(path, body)     { return req("POST", path, body); },
    async put(path, body)      { return req("PUT", path, body); },
    async del(path)            { return req("DELETE", path); },
};
async function req(method, path, body) {
    const opts = { method, headers: {} };
    if (body !== undefined) {
        opts.headers["Content-Type"] = "application/json";
        opts.body = JSON.stringify(body);
    }
    const r = await fetch(path, opts);
    const text = await r.text();
    let data = null;
    try { data = text ? JSON.parse(text) : null; } catch { data = { error: text }; }
    if (!r.ok) {
        const msg = (data && data.error) || `HTTP ${r.status}`;
        throw new Error(msg);
    }
    return data;
}

// ── Toast ──────────────────────────────────────────────────────────────
function toast(msg, kind = "ok") {
    const t = el("div", { class: `toast ${kind}` }, msg);
    $("#toast-stack").appendChild(t);
    setTimeout(() => {
        t.style.transition = "opacity .2s, transform .2s";
        t.style.opacity = "0";
        t.style.transform = "translateX(20px)";
        setTimeout(() => t.remove(), 220);
    }, 3200);
}

// ── Tabs ───────────────────────────────────────────────────────────────
function bindTabs() {
    $$(".tab").forEach(btn => btn.addEventListener("click", () => {
        $$(".tab").forEach(b => b.classList.toggle("active", b === btn));
        const target = btn.dataset.tab;
        $$(".tab-panel").forEach(p => p.classList.toggle("active", p.id === `tab-${target}`));
    }));
}

// ── Status polling ─────────────────────────────────────────────────────
const statusUI = {
    label:  () => $("#status-label"),
    dot:    () => $(".status-pill .dot"),
    uptime: () => $("#status-uptime"),
    btnStart: () => $("#btn-start"),
    btnStop:  () => $("#btn-stop"),
};

function fmtUptime(secs) {
    if (secs == null) return "";
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = secs % 60;
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
}

async function refreshStatus() {
    try {
        const s = await api.get("/api/status");
        statusUI.label().textContent = s.status;
        statusUI.dot().dataset.status = s.status;
        statusUI.uptime().textContent = s.uptime_seconds != null
            ? `· up ${fmtUptime(s.uptime_seconds)}`
            : "";
        const alive = ["starting","running","stopping"].includes(s.status);
        statusUI.btnStart().disabled = alive || !s.paths_ok;
        statusUI.btnStop().disabled  = !alive;
        updateTelnetPill(!!s.telnet_attached);
    } catch (e) {
        statusUI.label().textContent = "offline";
        statusUI.dot().dataset.status = "stopped";
        updateTelnetPill(false);
    }
}

// ── Lifecycle buttons ──────────────────────────────────────────────────
function bindLifecycleButtons() {
    statusUI.btnStart().addEventListener("click", async () => {
        try { await api.post("/api/start"); toast("server starting…"); }
        catch (e) { toast(e.message, "error"); }
        refreshStatus();
    });
    statusUI.btnStop().addEventListener("click", async () => {
        try { await api.post("/api/stop"); toast("stop requested"); }
        catch (e) { toast(e.message, "error"); }
        refreshStatus();
    });
}

// ── Server-config editor ───────────────────────────────────────────────
let configState = { props: [], dirty: new Map() };

async function loadConfig() {
    const groups = $("#settings-groups");
    groups.innerHTML = `<p class="placeholder">loading server settings…</p>`;
    try {
        const cfg = await api.get("/api/config");
        configState.props = cfg.properties.slice();
        configState.dirty.clear();
        renderConfig();
        // Also populate world tab from same data.
        populateWorldTab(cfg.properties);
    } catch (e) {
        groups.innerHTML = `<p class="placeholder">unable to load config: ${escapeHtml(e.message)}</p>`;
    }
}

function classify(propName) {
    for (const g of KNOWN_GROUPS) {
        if (g.match.includes(propName)) return g.name;
    }
    return "Misc / Advanced";
}

function renderConfig() {
    const root = $("#settings-groups");
    root.innerHTML = "";
    const filter = $("#settings-search").value.trim().toLowerCase();

    // bucket
    const buckets = new Map();
    KNOWN_GROUPS.forEach(g => buckets.set(g.name, []));
    buckets.set("Misc / Advanced", []);
    for (const p of configState.props) {
        const grp = classify(p.name);
        if (filter && !p.name.toLowerCase().includes(filter)) continue;
        buckets.get(grp).push(p);
    }

    let any = false;
    for (const [groupName, items] of buckets) {
        if (items.length === 0) continue;
        any = true;
        const groupEl = el("section", { class: "settings-group" });
        const header = el("header", {}, groupName, el("span", { class: "muted" }, `${items.length} props`));
        header.addEventListener("click", () => groupEl.classList.toggle("collapsed"));
        groupEl.appendChild(header);

        const grid = el("div", { class: "grid" });
        for (const p of items) grid.appendChild(renderProp(p));
        groupEl.appendChild(grid);
        root.appendChild(groupEl);
    }
    if (!any) root.innerHTML = `<p class="placeholder">no properties match.</p>`;
}

function renderProp(p) {
    const wrap = el("div", { class: "prop", dataset: { name: p.name } });
    wrap.appendChild(el("div", { class: "name", title: p.name }, p.name));
    const cell = el("div", { class: "value-cell" });

    let input;
    if (KNOWN_ENUMS[p.name]) {
        input = el("select");
        for (const [val, label] of KNOWN_ENUMS[p.name]) {
            const o = el("option", { value: val }, label);
            if (String(p.value) === val) o.selected = true;
            input.appendChild(o);
        }
    } else if (KNOWN_BOOLS.has(p.name)) {
        input = el("select");
        for (const [val, label] of [["true","true"],["false","false"]]) {
            const o = el("option", { value: val }, label);
            if (String(p.value).toLowerCase() === val) o.selected = true;
            input.appendChild(o);
        }
    } else if (KNOWN_NUMS.has(p.name)) {
        input = el("input", { type: "number", value: p.value });
    } else {
        input = el("input", { type: "text", value: p.value });
    }

    input.addEventListener("input", () => {
        configState.dirty.set(p.name, input.value);
        wrap.classList.add("dirty");
    });
    cell.appendChild(input);
    wrap.appendChild(cell);
    return wrap;
}

async function saveConfig() {
    // Merge dirty values into the full property list.
    const merged = configState.props.map(p =>
        configState.dirty.has(p.name)
            ? { name: p.name, value: configState.dirty.get(p.name) }
            : p);
    try {
        await api.put("/api/config", { properties: merged });
        toast("settings saved", "ok");
        configState.dirty.clear();
        await loadConfig();
        // ServerPort might have changed — refresh the share-with-friends card.
        loadConnectionInfo();
    } catch (e) {
        toast(`save failed: ${e.message}`, "error");
    }
}

// ── World / seed tab ───────────────────────────────────────────────────
async function populateWorldTab(props) {
    const get = (n) => (props.find(p => p.name === n) || {}).value;
    const sel = $("#world-select");
    sel.innerHTML = "";
    let maps = { maps: [] };
    try { maps = await api.get("/api/maps"); } catch {}
    for (const m of maps.maps) {
        sel.appendChild(el("option", { value: m.name }, `${m.name} ${m.kind === "generated" ? "(generated)" : ""}`));
    }
    if (get("GameWorld")) sel.value = get("GameWorld");
    $("#world-seed").value = get("WorldGenSeed") || "";
    $("#world-size").value = get("WorldGenSize") || 8192;
    $("#game-name").value  = get("GameName") || "";
}

async function rollSeeds() {
    const ul = $("#seed-vault");
    ul.innerHTML = `<li class="muted">rolling…</li>`;
    try {
        const data = await api.get("/api/seed?count=8");
        ul.innerHTML = "";
        for (const s of data.seeds) {
            const btn = el("button", {}, s);
            btn.addEventListener("click", () => {
                $("#world-seed").value = s;
                $$("#seed-vault button").forEach(b => b.classList.toggle("active", b === btn));
                toast(`seed set: ${s}`);
            });
            ul.appendChild(el("li", {}, btn));
        }
    } catch (e) {
        ul.innerHTML = `<li class="muted">error: ${escapeHtml(e.message)}</li>`;
    }
}

async function applyWorld() {
    const updates = [
        { name: "GameWorld",    value: $("#world-select").value },
        { name: "WorldGenSeed", value: $("#world-seed").value || "" },
        { name: "WorldGenSize", value: $("#world-size").value || "8192" },
        { name: "GameName",     value: $("#game-name").value || "" },
    ];
    // Merge into existing.
    const map = new Map(configState.props.map(p => [p.name, p.value]));
    for (const u of updates) map.set(u.name, u.value);
    const merged = [...map.entries()].map(([name, value]) => ({ name, value }));
    try {
        await api.put("/api/config", { properties: merged });
        toast("world applied to config", "ok");
        await loadConfig();
    } catch (e) {
        toast(`apply failed: ${e.message}`, "error");
    }
}

// ── Logs ───────────────────────────────────────────────────────────────
let logsSince = 0;
async function pollLogs() {
    try {
        const data = await api.get(`/api/logs?since=${logsSince}`);
        if (data.lines && data.lines.length) {
            const stream = $("#log-stream");
            const auto = $("#logs-autoscroll").checked;
            const frag = document.createDocumentFragment();
            for (const line of data.lines) {
                const ts = new Date(line.at).toLocaleTimeString();
                const lineEl = el("div");
                lineEl.appendChild(el("span", { class: "ts" }, ts));
                lineEl.appendChild(el("span", { class: line.kind }, line.line));
                frag.appendChild(lineEl);
            }
            stream.appendChild(frag);
            // Trim to ~2000 lines so the DOM doesn't balloon.
            while (stream.childElementCount > 2000) stream.removeChild(stream.firstChild);
            if (auto) stream.scrollTop = stream.scrollHeight;
        }
        logsSince = data.next_since;
    } catch {
        // tolerate transient errors silently
    }
}

// ── Presets ────────────────────────────────────────────────────────────
async function refreshPresets() {
    try {
        const names = await api.get("/api/presets");
        const ul = $("#preset-list");
        ul.innerHTML = "";
        if (!names.length) {
            ul.appendChild(el("li", { class: "muted" }, "no presets yet"));
            return;
        }
        for (const n of names) {
            const li = el("li", {},
                el("span", { class: "name" }, n),
                el("button", {
                    class: "btn ghost",
                    onclick: async () => {
                        try { await api.post(`/api/presets/${encodeURIComponent(n)}`); toast(`applied "${n}"`); await loadConfig(); }
                        catch (e) { toast(e.message, "error"); }
                    },
                }, "apply"),
                el("button", {
                    class: "btn ghost",
                    onclick: async () => {
                        try {
                            const res = await api.post(`/api/presets/${encodeURIComponent(n)}/export`);
                            toast(`exported to ${res.path}`, "ok");
                        } catch (e) { toast(e.message, "error"); }
                    },
                }, "export"),
                el("button", {
                    class: "btn ghost",
                    onclick: async () => {
                        if (!confirm(`Delete preset "${n}"?`)) return;
                        try { await api.del(`/api/presets/${encodeURIComponent(n)}`); refreshPresets(); }
                        catch (e) { toast(e.message, "error"); }
                    },
                }, "delete"),
            );
            ul.appendChild(li);
        }
    } catch (e) { /* shrug */ }
}

async function savePreset() {
    const name = $("#preset-name").value.trim();
    if (!name) { toast("preset name required", "error"); return; }
    try {
        await api.post("/api/presets", { name });
        $("#preset-name").value = "";
        toast(`saved preset "${name}"`, "ok");
        refreshPresets();
    } catch (e) { toast(e.message, "error"); }
}

async function importPreset() {
    const name = $("#preset-import-name").value.trim();
    const params = new URLSearchParams();
    if (name) params.set("name", name);
    try {
        const url = "/api/presets/import" + (params.toString() ? `?${params}` : "");
        const p = await api.post(url);
        $("#preset-import-name").value = "";
        toast(`imported preset "${p.name}"`, "ok");
        refreshPresets();
    } catch (e) {
        // "import cancelled" is a 400 — don't shout about it.
        if (!/cancel/i.test(e.message)) toast(e.message, "error");
    }
}

// ── Paths tab ──────────────────────────────────────────────────────────
async function loadPaths() {
    try {
        const s = await api.get("/api/settings");
        $("#path-install").value = s.server_install_dir || "";
        $("#path-config").value  = s.server_config_path || "";
        $("#path-saves").value   = s.saves_dir || "";
        $("#path-worlds").value  = s.generated_worlds_dir || "";
        const bp = $("#path-backups");
        if (bp) bp.value = s.backup_dir || "";
    } catch (e) { toast(e.message, "error"); }
}

async function savePaths() {
    const body = {
        server_install_dir:    $("#path-install").value || null,
        server_config_path:    $("#path-config").value  || null,
        saves_dir:             $("#path-saves").value   || null,
        generated_worlds_dir:  $("#path-worlds").value  || null,
        backup_dir:            $("#path-backups")?.value || null,
    };
    try {
        await api.put("/api/settings", body);
        toast("paths saved", "ok");
        refreshStatus();
        loadConfig();
        loadBackups();
    } catch (e) { toast(e.message, "error"); }
}

// ── Helpers ────────────────────────────────────────────────────────────
function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, c => ({
        "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;"
    }[c]));
}

// ── Boot ───────────────────────────────────────────────────────────────
document.addEventListener("DOMContentLoaded", () => {
    bindTabs();
    bindLifecycleButtons();

    $("#btn-save-config").addEventListener("click", saveConfig);
    $("#btn-reload-config").addEventListener("click", loadConfig);
    $("#settings-search").addEventListener("input", renderConfig);

    $("#btn-roll-seeds").addEventListener("click", rollSeeds);
    $("#btn-randomize-seed").addEventListener("click", async () => {
        const data = await api.get("/api/seed?count=1");
        $("#world-seed").value = data.seeds[0];
    });
    $("#btn-apply-world").addEventListener("click", applyWorld);

    $("#btn-save-preset").addEventListener("click", savePreset);
    $("#btn-import-preset").addEventListener("click", importPreset);

    $("#btn-save-paths").addEventListener("click", savePaths);

    $("#btn-clear-logs").addEventListener("click", () => {
        $("#log-stream").innerHTML = "";
    });

    // New tab bindings (added when paid-host parity features were added).
    bindConsole();
    bindAdminControls();
    bindBackupControls();
    bindScheduleControls();
    bindPathPickers();
    bindTheme();
    bindUpnp();
    bindVpnGuides();
    bindWorldTransfer();
    bindModUpload();
    bindFirewall();

    // Initial loads.
    refreshStatus();
    loadConfig();
    loadPaths();
    refreshPresets();
    rollSeeds();
    pollLogs();
    loadAdmin();
    loadBackups();
    loadSchedule();
    loadVpnAdapters();
    loadConnectionInfo();
    loadWorlds();
    loadMods();
    loadFirewallStatus();

    // Polling.
    setInterval(refreshStatus, 2000);
    setInterval(pollLogs,      1500);
});

// ─── Console command input ─────────────────────────────────────────────
async function sendConsole(cmd) {
    if (!cmd.trim()) return;
    try {
        await api.post("/api/console/exec", { command: cmd });
    } catch (e) { toast(e.message, "error"); }
}
function bindConsole() {
    $("#console-form").addEventListener("submit", async (e) => {
        e.preventDefault();
        const input = $("#console-cmd");
        const cmd = input.value;
        input.value = "";
        await sendConsole(cmd);
    });
    // ↑/↓ history.
    const history = [];
    let cursor = -1;
    $("#console-cmd").addEventListener("keydown", (e) => {
        const input = e.currentTarget;
        if (e.key === "Enter" && input.value) {
            history.push(input.value);
            cursor = history.length;
        } else if (e.key === "ArrowUp" && history.length) {
            e.preventDefault();
            cursor = Math.max(0, cursor - 1);
            input.value = history[cursor] || "";
        } else if (e.key === "ArrowDown" && history.length) {
            e.preventDefault();
            cursor = Math.min(history.length, cursor + 1);
            input.value = history[cursor] || "";
        }
    });
}

// ─── Players & Admins ──────────────────────────────────────────────────
let adminFile = null;

async function loadAdmin() {
    try {
        adminFile = await api.get("/api/admin");
    } catch (e) {
        adminFile = { admins: [], whitelist: [], blacklist: [], permissions: [] };
    }
    renderAdmin();
}

function renderAdmin() {
    if (!adminFile) return;
    renderUserList("#admin-list",     adminFile.admins,    { showLevel: true,  empty: "no admins yet" });
    renderUserList("#whitelist-list", adminFile.whitelist, { showLevel: false, empty: "whitelist empty (server is open)" });
    renderUserList("#ban-list",       adminFile.blacklist, { showLevel: false, showReason: true, empty: "no bans" });
    renderPermList("#perm-list",      adminFile.permissions);
}

function renderUserList(sel, users, opts) {
    const ul = $(sel);
    ul.innerHTML = "";
    if (!users || users.length === 0) {
        ul.appendChild(el("li", { class: "empty" }, opts.empty));
        return;
    }
    users.forEach((u, i) => {
        const li = el("li", {},
            el("span", { class: "platform" }, u.platform || "Steam"),
            el("span", { class: "id" }, u.user_id + (u.name ? `  (${u.name})` : "")),
            el("span", { class: "meta" },
                opts.showLevel ? `lvl ${u.permission_level ?? 0}` :
                opts.showReason && u.reason ? u.reason : ""),
            el("button", {
                class: "btn ghost",
                onclick: () => {
                    users.splice(i, 1);
                    renderAdmin();
                },
            }, "remove"),
        );
        ul.appendChild(li);
    });
}

function renderPermList(sel, perms) {
    const ul = $(sel);
    ul.innerHTML = "";
    if (!perms || perms.length === 0) {
        ul.appendChild(el("li", { class: "empty" }, "no overrides — game defaults apply"));
        return;
    }
    perms.forEach((p, i) => {
        const li = el("li", {},
            el("span", { class: "platform" }, "CMD"),
            el("span", { class: "id" }, p.cmd),
            el("span", { class: "meta" }, `lvl ${p.permission_level}`),
            el("button", {
                class: "btn ghost",
                onclick: () => { perms.splice(i, 1); renderAdmin(); },
            }, "remove"),
        );
        ul.appendChild(li);
    });
}

function bindAdminControls() {
    $("#btn-add-admin").addEventListener("click", () => {
        const id = $("#admin-userid").value.trim();
        if (!id) return toast("user id required", "error");
        adminFile.admins.push({
            platform: $("#admin-platform").value,
            user_id:  id,
            name:     $("#admin-name").value.trim() || null,
            permission_level: parseInt($("#admin-level").value, 10) || 0,
        });
        $("#admin-userid").value = ""; $("#admin-name").value = "";
        renderAdmin();
    });
    $("#btn-add-wl").addEventListener("click", () => {
        const id = $("#wl-userid").value.trim();
        if (!id) return toast("user id required", "error");
        adminFile.whitelist.push({
            platform: $("#wl-platform").value,
            user_id:  id,
            name:     $("#wl-name").value.trim() || null,
        });
        $("#wl-userid").value = ""; $("#wl-name").value = "";
        renderAdmin();
    });
    $("#btn-add-ban").addEventListener("click", () => {
        const id = $("#ban-userid").value.trim();
        if (!id) return toast("user id required", "error");
        adminFile.blacklist.push({
            platform: $("#ban-platform").value,
            user_id:  id,
            name:     $("#ban-name").value.trim() || null,
            reason:   $("#ban-reason").value.trim() || null,
        });
        $("#ban-userid").value = ""; $("#ban-name").value = "";
        $("#ban-reason").value = "";
        renderAdmin();
    });
    $("#btn-add-perm").addEventListener("click", () => {
        const cmd = $("#perm-cmd").value.trim();
        if (!cmd) return toast("command required", "error");
        adminFile.permissions.push({
            cmd,
            permission_level: parseInt($("#perm-level").value, 10) || 1000,
        });
        $("#perm-cmd").value = "";
        renderAdmin();
    });
    $("#btn-save-admin").addEventListener("click", async () => {
        try {
            await api.put("/api/admin", adminFile);
            toast("admin file saved", "ok");
        } catch (e) { toast(e.message, "error"); }
    });
}

// ─── Backups ───────────────────────────────────────────────────────────
function fmtSize(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
async function loadBackups() {
    try {
        const data = await api.get("/api/backups");
        const ul = $("#backup-list");
        ul.innerHTML = "";
        if (!data.backups.length) {
            ul.appendChild(el("li", { class: "user-list-empty muted" }, "no backups yet"));
            return;
        }
        for (const b of data.backups) {
            const ts = new Date(b.timestamp).toLocaleString();
            const li = el("li", {},
                el("span", { class: "ts" }, `${b.save_name} · ${ts}`),
                el("span", { class: "meta" }, fmtSize(b.size_bytes)),
                el("button", {
                    class: "btn ghost",
                    onclick: async () => {
                        if (!confirm("Restore this backup? Server must be stopped first.")) return;
                        try { await api.post("/api/backups/restore", { path: b.path }); toast("backup restored", "ok"); }
                        catch (e) { toast(e.message, "error"); }
                    },
                }, "restore"),
                el("button", {
                    class: "btn danger",
                    onclick: async () => {
                        if (!confirm("Delete this backup permanently?")) return;
                        try { await api.post("/api/backups/delete", { path: b.path }); loadBackups(); }
                        catch (e) { toast(e.message, "error"); }
                    },
                }, "delete"),
            );
            if (b.note) li.title = b.note;
            ul.appendChild(li);
        }
    } catch (e) { toast(e.message, "error"); }
}

function bindBackupControls() {
    $("#btn-create-backup").addEventListener("click", async () => {
        try {
            await api.post("/api/backups", { note: $("#backup-note").value || null });
            $("#backup-note").value = "";
            toast("backup created", "ok");
            loadBackups();
        } catch (e) { toast(e.message, "error"); }
    });
}

// ─── Schedule ──────────────────────────────────────────────────────────
async function loadSchedule() {
    try {
        const tasks = await api.get("/api/schedule");
        const ul = $("#schedule-list");
        ul.innerHTML = "";
        if (!tasks.length) {
            ul.appendChild(el("li", { class: "muted" }, "no scheduled tasks yet"));
            return;
        }
        for (const t of tasks) {
            const li = el("li", { class: t.enabled ? "" : "disabled" },
                el("span", { class: "time" }, t.at),
                el("span", { class: "action" }, t.action),
                el("span", { class: "name" }, t.name),
                el("button", {
                    class: "btn ghost",
                    onclick: async () => {
                        const updated = { ...t, enabled: !t.enabled };
                        await api.put("/api/schedule", updated);
                        loadSchedule();
                    },
                }, t.enabled ? "disable" : "enable"),
                el("button", {
                    class: "btn danger",
                    onclick: async () => {
                        if (!confirm(`Delete task '${t.name}'?`)) return;
                        await api.del(`/api/schedule/${encodeURIComponent(t.id)}`);
                        loadSchedule();
                    },
                }, "delete"),
            );
            ul.appendChild(li);
        }
    } catch (e) { toast(e.message, "error"); }
}

function bindScheduleControls() {
    $("#btn-add-sched").addEventListener("click", async () => {
        const task = {
            id: "",
            name: $("#sched-name").value.trim() || `${$("#sched-action").value} @ ${$("#sched-time").value}`,
            at: $("#sched-time").value,
            action: $("#sched-action").value,
            enabled: true,
            last_fired_iso: null,
        };
        try {
            await api.post("/api/schedule", task);
            $("#sched-name").value = "";
            loadSchedule();
            toast("task scheduled", "ok");
        } catch (e) { toast(e.message, "error"); }
    });
}

// ─── Telnet pill update ────────────────────────────────────────────────
function updateTelnetPill(attached) {
    const pill = $("#telnet-pill");
    if (!pill) return;
    pill.dataset.state = attached ? "on" : "off";
    pill.textContent = attached ? "telnet: on" : "telnet: off";
}

// ─── Native folder/file picker ─────────────────────────────────────────
function bindPathPickers() {
    $$("[data-pick]").forEach(btn => {
        btn.addEventListener("click", async (e) => {
            e.preventDefault();
            const targetId = btn.dataset.pick;
            const kind     = btn.dataset.pickKind || "dir";
            const title    = btn.dataset.pickTitle || "Select…";
            const target   = $("#" + targetId);
            const start    = target?.value || "";
            try {
                const params = new URLSearchParams({ kind, title });
                if (start) params.set("start", start);
                const res = await api.get(`/api/pick-path?${params}`);
                if (res && res.path) {
                    target.value = res.path;
                    // visual feedback
                    target.dispatchEvent(new Event("input", { bubbles: true }));
                }
            } catch (err) {
                toast(`picker failed: ${err.message}`, "error");
            }
        });
    });
}

// ─── Theme switcher ────────────────────────────────────────────────────
function bindTheme() {
    const sel = $("#theme-select");
    if (!sel) return;
    // Sync the dropdown to whatever the early inline script applied.
    let current = "default";
    try { current = localStorage.getItem("sdtd-theme") || "default"; } catch {}
    sel.value = current;

    sel.addEventListener("change", () => {
        const v = sel.value;
        if (v === "default") {
            document.documentElement.removeAttribute("data-theme");
        } else {
            document.documentElement.setAttribute("data-theme", v);
        }
        try { localStorage.setItem("sdtd-theme", v); } catch {}
    });
}

// ─── UPnP auto port forwarding ─────────────────────────────────────────
function bindUpnp() {
    const fwdBtn = $("#btn-upnp-forward");
    const unmapBtn = $("#btn-upnp-unmap");
    if (!fwdBtn || !unmapBtn) return;

    fwdBtn.addEventListener("click", async () => {
        renderUpnpStatus({ kind: "loading", msg: "discovering router via UPnP…" });
        fwdBtn.disabled = true;
        try {
            const r = await api.post("/api/upnp/forward");
            renderUpnpForwardResult(r);
            toast("port forwarding requested", "ok");
            loadConnectionInfo();
        } catch (e) {
            renderUpnpStatus({ kind: "bad", msg: e.message });
            toast(e.message, "error");
        } finally {
            fwdBtn.disabled = false;
        }
    });

    unmapBtn.addEventListener("click", async () => {
        if (!confirm("Remove the UPnP port mappings created by this tool?")) return;
        renderUpnpStatus({ kind: "loading", msg: "removing mappings…" });
        unmapBtn.disabled = true;
        try {
            const r = await api.post("/api/upnp/unmap");
            const lines = [
                row("removed", `${r.removed_ports.length} port(s)`, "success"),
            ];
            if (r.notes && r.notes.length) {
                lines.push(notesBlock(r.notes));
            }
            const el = $("#upnp-status");
            el.className = "upnp-status shown ok";
            el.innerHTML = lines.join("");
            toast("forwarding removed", "ok");
        } catch (e) {
            renderUpnpStatus({ kind: "bad", msg: e.message });
            toast(e.message, "error");
        } finally {
            unmapBtn.disabled = false;
        }
    });
}

function row(label, value, klass = "") {
    return `<div class="row-line">
        <span class="label">${label}</span>
        <span class="value ${klass}">${escapeHtml(value)}</span>
    </div>`;
}
function notesBlock(notes) {
    return `<div class="notes"><ul>${
        notes.map(n => `<li>${escapeHtml(n)}</li>`).join("")
    }</ul></div>`;
}
function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, c => ({
        "&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"
    }[c]));
}

function renderUpnpStatus({ kind, msg }) {
    const elx = $("#upnp-status");
    elx.className = "upnp-status shown " + (kind === "loading" ? "" : kind);
    elx.innerHTML = row("status", msg, kind === "bad" ? "failure" : "");
}

function renderUpnpForwardResult(r) {
    const lines = [];

    // Mapped ports
    lines.push(row(
        "mapped ports",
        r.mapped_ports.length ? r.mapped_ports.join(", ") : "(none)",
        r.mapped_ports.length ? "success" : "failure",
    ));

    // Local IP (this machine on the LAN)
    if (r.local_ip) lines.push(row("local IP", r.local_ip));

    // Public IP from the router's WAN side
    if (r.public_ip) {
        lines.push(row(
            "public IP",
            r.public_ip,
            r.cgnat ? "warning" : "success",
        ));
    } else {
        lines.push(row("public IP", "(could not query router)", "warning"));
    }

    // CGNAT verdict
    if (r.cgnat) {
        lines.push(row("CGNAT", "yes — outside players cannot reach you", "failure"));
    } else if (r.public_ip) {
        lines.push(row("CGNAT", "no", "success"));
    }

    // Friends-can-connect summary
    const reachable = r.mapped_ports.length > 0 && !r.cgnat && !!r.public_ip;
    lines.push(row(
        "friends connect",
        reachable
          ? `yes — give them ${r.public_ip}:${r.mapped_ports[0]}`
          : "no — see notes below",
        reachable ? "success" : "failure",
    ));

    if (r.notes && r.notes.length) lines.push(notesBlock(r.notes));

    const elx = $("#upnp-status");
    let klass = "ok";
    if (r.cgnat || r.mapped_ports.length === 0) klass = "bad";
    else if (r.notes && r.notes.length)         klass = "warn";
    elx.className = "upnp-status shown " + klass;
    elx.innerHTML = lines.join("");
}

// ─── VPN / virtual LAN fallback ────────────────────────────────────────
//
// When port forwarding doesn't work (CGNAT etc.), users can use a
// virtual LAN tool. We:
//   - detect any installed/running VPN adapter and surface its IP
//   - show step-by-step setup guides for each supported tool

const VPN_GUIDES = {
    radmin: {
        title: "Radmin VPN",
        steps: [
            "Download Radmin VPN on this computer (the server) AND on each friend's computer.",
            "Install and open it on every machine.",
            "On <strong>your</strong> (server) machine, click <strong>Network → Create network</strong>. Pick a name and password.",
            "Share the network name and password with your friends.",
            "Each friend clicks <strong>Network → Join an existing network</strong> and enters those.",
            "Once everyone shows green/online in Radmin, your <code>ServerPort</code> in <code>serverconfig.xml</code> is fine as-is — friends connect to your <strong>Radmin IP (26.x.x.x)</strong>, NOT your real public IP.",
            "In game: friends pick <strong>Connect to Server → IP</strong> and type your Radmin IP plus the server port.",
        ],
    },
    hamachi: {
        title: "Hamachi (LogMeIn)",
        steps: [
            "Install Hamachi on this computer (the server) AND on each friend's computer.",
            "Sign up / log in with a free LogMeIn account on each machine.",
            "On <strong>your</strong> machine, click <strong>Network → Create a new network</strong>. Pick a name and password (max 5 members on the free tier).",
            "Share the network name and password with your friends.",
            "Friends click <strong>Network → Join an existing network</strong> with those credentials.",
            "Friends connect to your <strong>Hamachi IP (25.x.x.x)</strong> + server port. Don't share your real public IP.",
        ],
    },
    tailscale: {
        title: "Tailscale",
        steps: [
            "Install Tailscale on this computer AND on each friend's computer.",
            "Each person logs in with their own account (Google / Microsoft / GitHub work).",
            "Add each friend's account to <strong>your</strong> Tailnet via the Tailscale admin console (or use a shared invite link).",
            "Once everyone is on the same Tailnet, your machine has a Tailscale IP shown in the app (a 100.x.x.x address) and a clean MagicDNS name.",
            "Friends connect to that <strong>Tailscale IP or MagicDNS name</strong> + your server port.",
            "Tailscale is direct peer-to-peer (no relay overhead) most of the time, so latency should be similar to a real LAN.",
        ],
    },
    zerotier: {
        title: "ZeroTier",
        steps: [
            "Sign up at <strong>my.zerotier.com</strong>. Create a network — note the 16-character <strong>Network ID</strong>.",
            "Install ZeroTier One on this computer AND on each friend's computer.",
            "On every machine, click the system-tray icon → <strong>Join New Network</strong>, paste the Network ID.",
            "Back in <strong>my.zerotier.com</strong>, authorize each member that just joined (checkbox in the member list).",
            "Each authorized machine gets an IP from your ZeroTier network's range (default <code>10.147.x.x</code>).",
            "Friends connect to your ZeroTier IP + server port.",
        ],
    },
};

async function loadVpnAdapters() {
    try {
        const adapters = await api.get("/api/vpn-adapters");
        const box = $("#vpn-detected");
        box.innerHTML = "";
        if (!adapters || !adapters.length) {
            box.classList.remove("shown");
            return;
        }
        box.classList.add("shown");
        const intro = el("p", { class: "muted small" },
            "We found a virtual LAN adapter on this machine. Share this IP + your server port with friends:");
        box.appendChild(intro);
        for (const a of adapters) {
            const row = el("div", { class: "vpn-card" },
                el("div", {},
                    el("div", { class: "vpn-name" }, a.display_name),
                    el("div", { class: "vpn-ip" }, a.ip),
                    el("div", { class: "vpn-meta" }, a.adapter_label),
                ),
                el("button", {
                    class: "btn ghost copy-btn",
                    onclick: async (e) => {
                        try {
                            await navigator.clipboard.writeText(a.ip);
                            e.currentTarget.textContent = "✓ copied";
                            setTimeout(() => { e.currentTarget.textContent = "copy IP"; }, 1500);
                        } catch (err) {
                            toast(`could not copy: ${err.message}`, "error");
                        }
                    },
                }, "copy IP"),
            );
            box.appendChild(row);
        }
    } catch (e) {
        // Silent — this is purely informational, no need to nag.
        console.warn("vpn detect failed", e);
    }
}

function bindVpnGuides() {
    $$("[data-vpn-help]").forEach(btn => {
        btn.addEventListener("click", () => {
            const key = btn.dataset.vpnHelp;
            const guide = VPN_GUIDES[key];
            if (!guide) return;
            const help = $("#vpn-help");
            help.innerHTML = "";
            help.appendChild(el("h4", {}, `${guide.title} — setup`));
            const ol = el("ol", {});
            for (const step of guide.steps) {
                const li = document.createElement("li");
                li.innerHTML = step; // safe: content is hard-coded above
                ol.appendChild(li);
            }
            help.appendChild(ol);
            help.classList.add("shown");
            help.scrollIntoView({ behavior: "smooth", block: "nearest" });
        });
    });

    // Re-scan adapters every time the fallback section opens — the user
    // might have installed and started a VPN since the page loaded.
    const fallback = document.querySelector(".vpn-fallback");
    if (fallback) {
        fallback.addEventListener("toggle", () => {
            if (fallback.open) loadVpnAdapters();
        });
    }
}

// ─── Connection info — "share these with friends" card ────────────────
//
// Pulls every connectable address (public IP, LAN IP, VPN IPs) from
// /api/connection-info and renders them with copy buttons. Refreshed
// when config changes (new ServerPort) or after UPnP forward succeeds.

async function loadConnectionInfo() {
    const box = $("#connect-list");
    if (!box) return;
    try {
        const info = await api.get("/api/connection-info");
        renderConnectionInfo(info, box);
    } catch (e) {
        box.innerHTML = "";
        box.appendChild(el("div", { class: "connect-empty" },
            `could not gather connection info: ${e.message}`));
    }
}

function renderConnectionInfo(info, box) {
    box.innerHTML = "";

    // Without a port we can't form a usable "ip:port" — show what we
    // have but make clear the in-game box needs a port.
    const port = info.port;

    if (!info.endpoints || info.endpoints.length === 0) {
        box.appendChild(el("div", { class: "connect-empty" },
            "could not detect any usable IPs. Set ServerPort in serverconfig.xml " +
            "and make sure your network is up."));
        return;
    }

    for (const ep of info.endpoints) {
        const fullAddr = port ? `${ep.ip}:${port}` : ep.ip;
        const isWarn = ep.note && /CGNAT|cannot reach|can't reach/i.test(ep.note);

        const row = el("div", { class: `connect-row kind-${ep.kind}${isWarn ? " warn" : ""}` },
            el("div", {},
                el("div", { class: "label" }, ep.label),
                el("div", { class: "addr" }, fullAddr),
                ep.note ? el("div", { class: "note" }, ep.note) : null,
            ),
            el("div", { class: "copy-stack" },
                copyBtn("copy ip:port", fullAddr),
                port ? copyBtn("copy IP only", ep.ip) : null,
            ),
        );
        box.appendChild(row);
    }
}

function copyBtn(label, text) {
    return el("button", {
        class: "btn ghost",
        onclick: async (e) => {
            const btn = e.currentTarget;
            const original = label;
            try {
                await navigator.clipboard.writeText(text);
                btn.textContent = "✓ copied";
            } catch {
                // Older browsers / non-HTTPS contexts: fall back to a
                // hidden textarea + execCommand.
                const ta = document.createElement("textarea");
                ta.value = text;
                ta.style.position = "fixed"; ta.style.opacity = "0";
                document.body.appendChild(ta);
                ta.select();
                try { document.execCommand("copy"); btn.textContent = "✓ copied"; }
                catch { btn.textContent = "copy failed"; }
                ta.remove();
            }
            setTimeout(() => { btn.textContent = original; }, 1500);
        },
    }, label);
}

// ─── World transfer (download/upload zips) ─────────────────────────────
//
// Download is just a window.location to /api/worlds/download/<n> —
// the browser handles the streaming save dialog natively.
//
// Upload uses XMLHttpRequest because we want progress events that
// fetch() doesn't expose. We also chunk-stream in a way that lets the
// browser surface "uploading 23%" as the file's bytes go up the wire.

function fmtSizeBytes(b) {
    if (b < 1024) return `${b} B`;
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
    if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
    return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

async function loadWorlds() {
    const ul = $("#world-list");
    if (!ul) return;
    try {
        const worlds = await api.get("/api/worlds");
        ul.innerHTML = "";
        if (!worlds.length) {
            ul.appendChild(el("li", { class: "empty muted" },
                "no worlds found. Set GameName and start the server, " +
                "or upload a zip below."));
            return;
        }
        for (const w of worlds) {
            const ts = w.modified_iso ? new Date(w.modified_iso).toLocaleString() : "";
            const li = el("li", {},
                el("div", {},
                    el("div", { class: "name" }, w.name),
                    el("div", { class: "meta" }, `${fmtSizeBytes(w.size_bytes)}${ts ? ` · ${ts}` : ""}`),
                ),
                el("button", {
                    class: "btn ghost",
                    onclick: () => downloadWorld(w.name),
                }, "↓ download zip"),
                el("button", {
                    class: "btn danger",
                    onclick: async () => {
                        if (!confirm(`Delete world "${w.name}"?\n\nThis is permanent. Use BACKUPS for a recoverable snapshot instead. The server must be stopped.`)) return;
                        try {
                            const r = await api.del(`/api/worlds/${encodeURIComponent(w.name)}`);
                            toast(`deleted "${r.name}" (${fmtSizeBytes(r.bytes_freed)} freed)`, "ok");
                            await loadWorlds();
                            if (typeof loadConfig === "function") loadConfig();
                        } catch (e) {
                            toast(e.message, "error");
                        }
                    },
                }, "delete"),
            );
            ul.appendChild(li);
        }
    } catch (e) {
        ul.innerHTML = "";
        ul.appendChild(el("li", { class: "empty muted" },
            `could not load worlds: ${e.message}`));
    }
}

function downloadWorld(name) {
    // Trigger a native browser download. Streaming happens server-side;
    // the browser shows its own progress bar.
    const url = `/api/worlds/download/${encodeURIComponent(name)}`;
    const a = document.createElement("a");
    a.href = url;
    a.download = `${name}.zip`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    toast("preparing download…", "ok");
}

function bindWorldTransfer() {
    const btn = $("#btn-upload-world");
    if (!btn) return;
    btn.addEventListener("click", uploadWorld);
}

async function uploadWorld() {
    const fileInput = $("#upload-file");
    const file = fileInput.files && fileInput.files[0];
    if (!file) {
        toast("pick a .zip file first", "error");
        return;
    }
    if (!file.name.toLowerCase().endsWith(".zip")) {
        if (!confirm(`"${file.name}" doesn't look like a .zip. Try anyway?`)) return;
    }

    const fd = new FormData();
    fd.append("file", file);
    const nameOverride = $("#upload-name").value.trim();
    if (nameOverride) fd.append("name", nameOverride);
    if ($("#upload-overwrite").checked) fd.append("overwrite", "true");

    const prog = $("#upload-progress");
    prog.className = "upload-progress shown";
    prog.innerHTML = `
        <div class="status">uploading ${escapeHtml(file.name)} · ${fmtSizeBytes(file.size)}</div>
        <div class="bar"><div class="fill"></div></div>
    `;
    const fill = prog.querySelector(".fill");
    const status = prog.querySelector(".status");

    // Disable the button so the user doesn't double-submit during a
    // multi-minute upload.
    const btn = $("#btn-upload-world");
    btn.disabled = true;

    try {
        const result = await xhrUpload("/api/worlds/upload", fd, (loaded, total) => {
            if (total) {
                const pct = (loaded / total) * 100;
                fill.style.width = `${pct.toFixed(1)}%`;
                status.textContent = `uploading… ${fmtSizeBytes(loaded)} / ${fmtSizeBytes(total)} (${pct.toFixed(0)}%)`;
            } else {
                status.textContent = `uploading… ${fmtSizeBytes(loaded)}`;
            }
        });
        // Server-side extraction happens after the body finishes uploading;
        // the response carries the result.
        prog.className = "upload-progress shown ok";
        fill.style.width = "100%";
        status.textContent =
            `done — extracted "${result.save_name}" (${result.file_count} files, ${fmtSizeBytes(result.bytes_extracted)})`;
        toast(`uploaded "${result.save_name}"`, "ok");
        // Refresh the world list and the world dropdown in the same tab.
        await loadWorlds();
        if (typeof loadConfig === "function") loadConfig();
    } catch (e) {
        prog.className = "upload-progress shown bad";
        status.textContent = `failed: ${e.message}`;
        toast(e.message, "error");
    } finally {
        btn.disabled = false;
    }
}

/// Promise wrapper around XHR — we want upload progress, which
/// fetch() doesn't surface in any cross-browser way as of 2026.
function xhrUpload(url, formData, onProgress) {
    return new Promise((resolve, reject) => {
        const xhr = new XMLHttpRequest();
        xhr.open("POST", url);
        xhr.responseType = "json";
        xhr.upload.addEventListener("progress", (e) => {
            if (e.lengthComputable && onProgress) onProgress(e.loaded, e.total);
        });
        xhr.addEventListener("load", () => {
            if (xhr.status >= 200 && xhr.status < 300) {
                resolve(xhr.response);
            } else {
                const msg = (xhr.response && xhr.response.error) || xhr.statusText || `HTTP ${xhr.status}`;
                reject(new Error(msg));
            }
        });
        xhr.addEventListener("error", () => reject(new Error("network error during upload")));
        xhr.addEventListener("abort", () => reject(new Error("upload aborted")));
        xhr.send(formData);
    });
}

// ─── Mods (modlets) ─────────────────────────────────────────────────────
async function loadMods() {
    const ul = $("#mod-list");
    if (!ul) return;
    try {
        const mods = await api.get("/api/mods");
        ul.innerHTML = "";
        if (!mods.length) {
            ul.appendChild(el("li", { class: "empty muted" },
                "no mods installed. Upload a zip below to add one."));
            return;
        }
        for (const m of mods) {
            const ts = m.modified_iso ? new Date(m.modified_iso).toLocaleString() : "";
            const flag = m.has_modinfo
                ? ""
                : " · ⚠ no ModInfo.xml";
            const li = el("li", {},
                el("div", {},
                    el("div", { class: "name" }, m.name),
                    el("div", { class: "meta" },
                        `${fmtSizeBytes(m.size_bytes)}${ts ? ` · ${ts}` : ""}${flag}`),
                ),
                el("button", {
                    class: "btn danger",
                    onclick: async () => {
                        if (!confirm(`Remove mod "${m.name}"?\n\nThis deletes the folder. The server must be stopped.`)) return;
                        try {
                            const r = await api.del(`/api/mods/${encodeURIComponent(m.name)}`);
                            toast(`removed "${r.name}" (${fmtSizeBytes(r.bytes_freed)} freed)`, "ok");
                            await loadMods();
                        } catch (e) {
                            toast(e.message, "error");
                        }
                    },
                }, "remove"),
            );
            ul.appendChild(li);
        }
    } catch (e) {
        ul.innerHTML = "";
        ul.appendChild(el("li", { class: "empty muted" },
            `could not load mods: ${e.message}`));
    }
}

function bindModUpload() {
    const btn = $("#btn-upload-mod");
    if (!btn) return;
    btn.addEventListener("click", uploadMod);
}

async function uploadMod() {
    const fileInput = $("#mod-upload-file");
    const file = fileInput.files && fileInput.files[0];
    if (!file) {
        toast("pick a .zip file first", "error");
        return;
    }
    if (!file.name.toLowerCase().endsWith(".zip")) {
        if (!confirm(`"${file.name}" doesn't look like a .zip. Try anyway?`)) return;
    }

    const fd = new FormData();
    fd.append("file", file);
    const nameOverride = $("#mod-upload-name").value.trim();
    if (nameOverride) fd.append("name", nameOverride);
    if ($("#mod-upload-overwrite").checked) fd.append("overwrite", "true");

    const prog = $("#mod-upload-progress");
    prog.className = "upload-progress shown";
    prog.innerHTML = `
        <div class="status">uploading ${escapeHtml(file.name)} · ${fmtSizeBytes(file.size)}</div>
        <div class="bar"><div class="fill"></div></div>
    `;
    const fill = prog.querySelector(".fill");
    const status = prog.querySelector(".status");

    const btn = $("#btn-upload-mod");
    btn.disabled = true;

    try {
        const result = await xhrUpload("/api/mods/upload", fd, (loaded, total) => {
            if (total) {
                const pct = (loaded / total) * 100;
                fill.style.width = `${pct.toFixed(1)}%`;
                status.textContent = `uploading… ${fmtSizeBytes(loaded)} / ${fmtSizeBytes(total)} (${pct.toFixed(0)}%)`;
            } else {
                status.textContent = `uploading… ${fmtSizeBytes(loaded)}`;
            }
        });
        prog.className = "upload-progress shown ok";
        fill.style.width = "100%";
        status.textContent =
            `installed "${result.mod_name}" (${result.layout}, ${result.file_count} files, ${fmtSizeBytes(result.bytes_extracted)})`;
        toast(`installed "${result.mod_name}"`, "ok");
        fileInput.value = "";
        $("#mod-upload-name").value = "";
        $("#mod-upload-overwrite").checked = false;
        await loadMods();
    } catch (e) {
        prog.className = "upload-progress shown bad";
        status.textContent = `failed: ${e.message}`;
        toast(e.message, "error");
    } finally {
        btn.disabled = false;
    }
}

// ─── Windows Firewall rule management ──────────────────────────────────
function bindFirewall() {
    const allowBtn  = $("#btn-fw-allow");
    const removeBtn = $("#btn-fw-remove");
    if (!allowBtn || !removeBtn) return;

    allowBtn.addEventListener("click", async () => {
        renderFwDetail({ kind: "loading", msg: "asking Windows Firewall to allow our ports…" });
        allowBtn.disabled = true;
        try {
            const r = await api.post("/api/firewall/allow");
            renderFwResult(r, "allowed");
            toast("firewall rules added", "ok");
            loadFirewallStatus();
        } catch (e) {
            renderFwDetail({ kind: "bad", msg: e.message });
            // 403 → elevation needed. Show a more actionable message.
            if (/administrator/i.test(e.message)) {
                toast("relaunch the manager as administrator (right-click → Run as administrator)", "error");
            } else {
                toast(e.message, "error");
            }
        } finally {
            allowBtn.disabled = false;
        }
    });

    removeBtn.addEventListener("click", async () => {
        if (!confirm("Remove the firewall rules added by this tool?")) return;
        renderFwDetail({ kind: "loading", msg: "removing rules…" });
        removeBtn.disabled = true;
        try {
            const r = await api.post("/api/firewall/remove");
            renderFwResult(r, "removed");
            toast("firewall rules removed", "ok");
            loadFirewallStatus();
        } catch (e) {
            renderFwDetail({ kind: "bad", msg: e.message });
            toast(e.message, "error");
        } finally {
            removeBtn.disabled = false;
        }
    });
}

async function loadFirewallStatus() {
    const pill = $("#fw-status");
    if (!pill) return;
    try {
        const s = await api.get("/api/firewall/status");
        if (s.unsupported) {
            pill.dataset.state = "off";
            pill.textContent = "firewall: not managed (non-Windows)";
            return;
        }
        if (s.any_present) {
            pill.dataset.state = "on";
            pill.textContent = `firewall: ${s.present.length} rule(s) active`;
        } else {
            pill.dataset.state = "off";
            pill.textContent = "firewall: no rules yet";
        }
    } catch (e) {
        pill.dataset.state = "off";
        pill.textContent = "firewall: unknown";
    }
}

function renderFwDetail({ kind, msg }) {
    const elx = $("#fw-detail");
    elx.className = "upnp-status shown " + (kind === "loading" ? "" : kind);
    elx.innerHTML = row("status", msg, kind === "bad" ? "failure" : "");
}

function renderFwResult(r, verb) {
    const lines = [];
    const ports = verb === "allowed" ? r.added_ports : r.removed_ports;
    lines.push(row(
        verb === "allowed" ? "added" : "removed",
        ports.length ? `${ports.length} port(s): ${ports.join(", ")}` : "(none)",
        ports.length ? "success" : "failure",
    ));
    if (r.notes && r.notes.length) lines.push(notesBlock(r.notes));
    const elx = $("#fw-detail");
    elx.className = "upnp-status shown " + (ports.length ? "ok" : "warn");
    elx.innerHTML = lines.join("");
}
