const invokeRaw = window.__TAURI_INTERNALS__
  ? (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
  : async () => {
      throw new Error("Tauri bridge unavailable");
    };

/** Frontend guard so a hung invoke can't leave spinners forever. */
function invokeWithTimeout(cmd, args, ms, label) {
  const timeoutMs = Math.max(1000, Number(ms) || 30000);
  let timer = null;
  const timeoutPromise = new Promise((_, reject) => {
    timer = setTimeout(() => {
      reject(
        new Error(
          label
            ? `${label} timed out — try again`
            : `Request timed out after ${Math.round(timeoutMs / 1000)}s`
        )
      );
    }, timeoutMs);
  });
  return Promise.race([invokeRaw(cmd, args), timeoutPromise]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

const invoke = (cmd, args) => invokeRaw(cmd, args);

const IMPORT_INVOKE_TIMEOUT_MS = 90000;

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => [...document.querySelectorAll(sel)];

/** @type {Array<any>} */
let historyAccounts = [];

function showAppModal({ title, body, confirmLabel = "Confirm", cancelLabel = "Cancel", danger = false }) {
  return new Promise((resolve) => {
    const root = $("#app-modal");
    const titleEl = $("#app-modal-title");
    const bodyEl = $("#app-modal-body");
    const confirmBtn = $("#app-modal-confirm");
    const cancelBtn = $("#app-modal-cancel");
    if (!root || !titleEl || !bodyEl || !confirmBtn || !cancelBtn) {
      resolve(false);
      return;
    }

    titleEl.textContent = title || "";
    bodyEl.textContent = body || "";
    confirmBtn.textContent = confirmLabel;
    cancelBtn.textContent = cancelLabel;
    confirmBtn.classList.toggle("danger", !!danger);
    confirmBtn.classList.toggle("primary", !danger);

    const finish = (value) => {
      root.hidden = true;
      confirmBtn.removeEventListener("click", onConfirm);
      cancelBtn.removeEventListener("click", onCancel);
      root.querySelectorAll("[data-modal-dismiss]").forEach((el) => {
        el.removeEventListener("click", onCancel);
      });
      document.removeEventListener("keydown", onKey);
      resolve(value);
    };
    const onConfirm = () => finish(true);
    const onCancel = () => finish(false);
    const onKey = (e) => {
      if (e.key === "Escape") onCancel();
    };

    confirmBtn.addEventListener("click", onConfirm);
    cancelBtn.addEventListener("click", onCancel);
    root.querySelectorAll("[data-modal-dismiss]").forEach((el) => {
      el.addEventListener("click", onCancel);
    });
    document.addEventListener("keydown", onKey);

    root.hidden = false;
    confirmBtn.focus();
  });
}

function toast(message, kind = "info") {
  const root = $("#toasts");
  const el = document.createElement("div");
  el.className = `toast ${kind === "ok" ? "ok" : kind === "err" ? "err" : ""}`;
  el.textContent = message;
  root.appendChild(el);
  setTimeout(() => el.remove(), 3000);
}

function setResult(el, text, ok) {
  el.hidden = !text;
  el.textContent = text || "";
  el.classList.toggle("ok", !!ok);
  el.classList.toggle("err", ok === false);
}

function formatOkBody(body) {
  if (!body || typeof body !== "object") return "Done";
  const parts = [];
  if (body.message) parts.push(String(body.message));
  if (body.status) parts.push(`Status: ${body.status}`);
  if (body.productName || body.product) {
    parts.push(`Product: ${body.productName || body.product}`);
  }
  if (body.ticket) parts.push(`Ticket: ${body.ticket}`);
  if (body.redeemed_at) parts.push(`Redeemed: ${body.redeemed_at}`);
  if (body.account) parts.push("Account delivered — import on Login");
  if (!parts.length && body.ok) parts.push("OK");
  return parts.join("\n");
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

async function handleOutcome(outcome, resultEl, { openOnDown } = {}) {
  if (outcome.site_down) {
    toast("Site not ready", "err");
    if (openOnDown) {
      try {
        await invoke("open_url", { url: openOnDown });
      } catch (_) {}
    }
    setResult(resultEl, "Site not ready", false);
    return null;
  }

  const body = outcome.body || {};
  if (body.ok === true) {
    const msg = formatOkBody(body);
    setResult(resultEl, msg, true);
    toast(body.message || "Success", "ok");
    return body;
  }

  let err =
    (typeof body.error === "string" && body.error) ||
    outcome.error ||
    "Request failed";

  if (err === "Unauthorized" && outcome.http_status === 401) {
    err = "Unauthorized (check appKey / APP_KEY)";
  } else if (/delivery service unauthorized/i.test(err)) {
    err = "Site delivery API key invalid — fix the provider API key on the redeem site";
  }

  setResult(resultEl, err, false);
  toast(err, "err");
  return null;
}

function switchTab(name) {
  $$(".tab").forEach((t) => {
    const on = t.dataset.tab === name;
    t.classList.toggle("active", on);
    t.setAttribute("aria-selected", on ? "true" : "false");
  });
  $$(".panel").forEach((p) => {
    const on = p.id === `panel-${name}`;
    p.classList.toggle("active", on);
    p.hidden = !on;
  });
  if (name === "history") loadHistory();
  if (name === "settings") refreshAdminSettings();
}

async function refreshAdminSettings() {
  const btn = $("#btn-run-as-admin");
  const status = $("#settings-admin-status");
  const hint = $("#settings-admin-hint");
  if (!btn || !status || !hint) return;
  try {
    const elevated = await invoke("is_running_as_admin");
    if (elevated) {
      btn.hidden = true;
      btn.disabled = true;
      status.hidden = false;
      status.textContent = "Running as administrator";
      hint.textContent = "Elevated — Steam close should work even when Access denied.";
    } else {
      btn.hidden = false;
      btn.disabled = false;
      status.hidden = true;
      hint.textContent = "Needed if Steam won't close (Access denied).";
    }
  } catch (_) {
    btn.hidden = false;
    btn.disabled = false;
    status.hidden = true;
  }
}

async function syncKeys(fromId) {
  const value = $(fromId).value.trim();
  ["#account-key", "#redeem-key", "#replacement-key"].forEach((id) => {
    if (id !== fromId) $(id).value = value;
  });
  if (value) {
    try {
      await invoke("save_last_key", { key: value });
    } catch (_) {}
  }
}

function clearLicenseKeys() {
  ["#account-key", "#redeem-key", "#replacement-key"].forEach((id) => {
    $(id).value = "";
  });
}

function renderHistory() {
  const list = $("#history-list");
  const empty = $("#history-empty");
  const rows = historyAccounts.slice();

  list.innerHTML = "";
  if (!rows.length) {
    empty.hidden = false;
    empty.textContent = "No accounts yet — import one on Login.";
    return;
  }
  empty.hidden = true;

  for (const a of rows) {
    const row = document.createElement("div");
    row.className = "hist-row";
    row.dataset.steamid = a.steamid;
    const canExport = !!a.hasToken;
    const sub = [
      a.accountName || a.steamid,
      a.hasToken ? "token saved" : "no token",
      a.mostRecent ? "active" : null,
    ]
      .filter(Boolean)
      .join(" · ");

    row.innerHTML = `
      <div class="hist-meta">
        <div class="hist-name">${escapeHtml(a.displayName || a.accountName || a.steamid)}</div>
        <div class="hist-sub">${escapeHtml(sub)}</div>
      </div>
      <div class="hist-actions">
        <button type="button" class="btn secondary" data-act="signin" data-id="${escapeHtml(a.steamid)}">Sign in</button>
        <button type="button" class="btn ghost" data-act="export" data-id="${escapeHtml(a.steamid)}" ${canExport ? "" : "disabled"} title="Copy username----token">Export</button>
        <button type="button" class="btn danger" data-act="remove" data-id="${escapeHtml(a.steamid)}">Remove</button>
      </div>
    `;
    list.appendChild(row);
  }

  list.querySelectorAll("[data-act]").forEach((btn) => {
    btn.addEventListener("click", () => onHistoryAction(btn.dataset.act, btn.dataset.id, btn));
  });
}

async function onHistoryAction(act, steamid, btn) {
  if (!steamid) return;
  btn.disabled = true;
  try {
    if (act === "signin") {
      const msg = await invokeWithTimeout(
        "sign_in",
        { steamid },
        IMPORT_INVOKE_TIMEOUT_MS,
        "Sign in"
      );
      toast(msg || "Signed in", "ok");
      await loadHistory();
    } else if (act === "export") {
      const msg = await invoke("export_account_token", { steamid });
      toast(msg || "Copied 1 account", "ok");
    } else if (act === "remove") {
      const ok = await showAppModal({
        title: "Remove account?",
        body: "Removes this account from History and Steam login files on this PC.",
        confirmLabel: "Remove",
        cancelLabel: "Cancel",
        danger: true,
      });
      if (!ok) return;
      const msg = await invoke("remove_account", { steamid });
      toast(msg || "Removed", "ok");
      await loadHistory();
    }
  } catch (e) {
    toast(String(e), "err");
  } finally {
    btn.disabled = false;
  }
}

async function loadHistory() {
  try {
    historyAccounts = (await invoke("list_accounts")) || [];
  } catch (e) {
    historyAccounts = [];
    toast(String(e), "err");
  }
  renderHistory();
}

async function exportAllHistory() {
  try {
    const msg = await invoke("export_all_account_tokens");
    toast(msg || "Copied", "ok");
  } catch (e) {
    toast(String(e), "err");
  }
}

async function clearAllHistory() {
  const ok = await showAppModal({
    title: "Clear all History?",
    body: "Clears saved accounts and Steam login cache on this PC. You’ll need to re-import tokens to sign in again.",
    confirmLabel: "Clear all",
    cancelLabel: "Cancel",
    danger: true,
  });
  if (!ok) return;

  const btn = $("#btn-hist-clear");
  if (btn) btn.disabled = true;
  try {
    const msg = await invoke("clear_all_history");
    historyAccounts = [];
    renderHistory();
    toast(msg || "Cleared", "ok");
  } catch (e) {
    toast(String(e), "err");
  } finally {
    if (btn) btn.disabled = false;
  }
}

function initStarfield() {
  const canvas = $("#starfield");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  let stars = [];
  let raf = 0;

  const resize = () => {
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    const count = Math.min(90, Math.floor((canvas.width * canvas.height) / 12000));
    stars = Array.from({ length: count }, () => ({
      x: Math.random() * canvas.width,
      y: Math.random() * canvas.height,
      r: Math.random() * 1.3 + 0.25,
      a: Math.random() * 0.5 + 0.15,
      speed: Math.random() * 0.12 + 0.02,
    }));
  };

  const draw = () => {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const g = ctx.createRadialGradient(
      canvas.width * 0.5,
      canvas.height * 0.2,
      30,
      canvas.width * 0.5,
      canvas.height * 0.4,
      canvas.width * 0.7
    );
    g.addColorStop(0, "rgba(91, 33, 182, 0.12)");
    g.addColorStop(0.45, "rgba(46, 16, 101, 0.06)");
    g.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    for (const s of stars) {
      s.y -= s.speed;
      if (s.y < -2) {
        s.y = canvas.height + 2;
        s.x = Math.random() * canvas.width;
      }
      ctx.beginPath();
      ctx.fillStyle = `rgba(226, 214, 255, ${s.a})`;
      ctx.arc(s.x, s.y, s.r, 0, Math.PI * 2);
      ctx.fill();
    }
    raf = requestAnimationFrame(draw);
  };

  resize();
  draw();
  window.addEventListener("resize", resize);
  return () => cancelAnimationFrame(raf);
}

async function boot() {
  initStarfield();

  const quitApp = () => invoke("quit_app");
  $("#btn-min").addEventListener("click", () => invoke("minimize_window"));
  $("#btn-close").addEventListener("click", quitApp);
  $("#btn-quit-app")?.addEventListener("click", quitApp);

  $("#btn-close-steam").addEventListener("click", async () => {
    const btn = $("#btn-close-steam");
    btn.disabled = true;
    try {
      await invoke("stop_steam");
      toast("Steam closed", "ok");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      btn.disabled = false;
    }
  });
  $("#btn-market").addEventListener("click", () =>
    invoke("open_url", { url: "https://nocheater.store" })
  );
  $("#btn-redeem-site").addEventListener("click", () =>
    invoke("open_url", { url: "https://sitnn.dog" })
  );
  $("#btn-discord").addEventListener("click", () =>
    invoke("open_url", { url: "https://nocheater.cc" })
  );

  $$(".tab").forEach((tab) => {
    tab.addEventListener("click", () => switchTab(tab.dataset.tab));
  });

  $("#btn-import").addEventListener("click", async () => {
    const line = $("#import-token").value.trim();
    const btn = $("#btn-import");
    btn.disabled = true;
    setResult($("#import-result"), "Closing Steam and importing…", null);
    try {
      if (line) {
        try {
          await invokeWithTimeout("save_last_token", { token: line }, 15000);
        } catch (_) {}
      }
      const msg = await invokeWithTimeout(
        "import_account",
        { line },
        IMPORT_INVOKE_TIMEOUT_MS,
        "Import"
      );
      setResult($("#import-result"), msg, true);
      toast(msg || "Imported", "ok");
      $("#import-token").value = "";
      await loadHistory();
    } catch (e) {
      setResult($("#import-result"), String(e), false);
      toast(String(e), "err");
    } finally {
      btn.disabled = false;
    }
  });

  $("#btn-import-clip").addEventListener("click", async () => {
    const btn = $("#btn-import-clip");
    btn.disabled = true;
    setResult($("#import-result"), "Importing from clipboard…", null);
    try {
      const msg = await invokeWithTimeout(
        "import_from_clipboard",
        {},
        IMPORT_INVOKE_TIMEOUT_MS,
        "Import"
      );
      setResult($("#import-result"), msg, true);
      toast(msg || "Imported", "ok");
      $("#import-token").value = "";
      await loadHistory();
    } catch (e) {
      setResult($("#import-result"), String(e), false);
      toast(String(e), "err");
    } finally {
      btn.disabled = false;
    }
  });

  $("#btn-status").addEventListener("click", async () => {
    const key = $("#account-key").value.trim();
    await syncKeys("#account-key");
    const btn = $("#btn-status");
    btn.disabled = true;
    try {
      const outcome = await invoke("get_key_status", { key });
      const body = await handleOutcome(outcome, $("#account-result"), {
        openOnDown: "https://nocheater.store",
      });
      if (body) clearLicenseKeys();
    } catch (e) {
      toast(String(e), "err");
    } finally {
      btn.disabled = false;
    }
  });

  $("#btn-hist-refresh").addEventListener("click", async () => {
    await loadHistory();
  });

  $("#btn-hist-export")?.addEventListener("click", () => {
    void exportAllHistory();
  });

  $("#btn-hist-clear")?.addEventListener("click", () => {
    void clearAllHistory();
  });

  $("#btn-redeem").addEventListener("click", async () => {
    const key = $("#redeem-key").value.trim();
    await syncKeys("#redeem-key");
    const btn = $("#btn-redeem");
    btn.disabled = true;
    try {
      const outcome = await invoke("redeem_key", { key });
      const body = await handleOutcome(outcome, $("#redeem-result"), {
        openOnDown: "https://nocheater.store",
      });
      if (body) {
        clearLicenseKeys();
        if (body.account) {
          const line = String(body.account);
          $("#import-token").value = line;
          try {
            await invoke("save_last_token", { token: line });
          } catch (_) {}
          toast("Delivered — import on Login", "ok");
        }
      }
    } catch (e) {
      toast(String(e), "err");
    } finally {
      btn.disabled = false;
    }
  });

  $("#btn-replacement").addEventListener("click", async () => {
    const key = $("#replacement-key").value.trim();
    const reason = $("#replacement-reason").value.trim();
    await syncKeys("#replacement-key");
    if (!reason) {
      toast("Reason is required", "err");
      setResult($("#replacement-result"), "Reason is required", false);
      return;
    }
    if (reason.length > 500) {
      toast("Reason must be 500 characters or fewer", "err");
      return;
    }
    const btn = $("#btn-replacement");
    btn.disabled = true;
    try {
      const outcome = await invoke("request_replacement", { key, reason });
      const body = await handleOutcome(outcome, $("#replacement-result"), {
        openOnDown: "https://nocheater.store",
      });
      if (body) {
        clearLicenseKeys();
        $("#replacement-reason").value = "";
      }
    } catch (e) {
      toast(String(e), "err");
    } finally {
      btn.disabled = false;
    }
  });

  const runAsAdminBtn = $("#btn-run-as-admin");
  if (runAsAdminBtn) {
    runAsAdminBtn.addEventListener("click", async () => {
      try {
        const elevated = await invoke("is_running_as_admin");
        if (elevated) {
          toast("Already running as administrator", "ok");
          await refreshAdminSettings();
          return;
        }
      } catch (_) {}

      runAsAdminBtn.disabled = true;
      try {
        await invoke("relaunch_as_admin");
        toast("Waiting for UAC…");
      } catch (e) {
        const msg = String(e);
        if (/already running as administrator/i.test(msg)) {
          toast("Already running as administrator", "ok");
          await refreshAdminSettings();
        } else {
          toast(msg || "Elevation cancelled", "err");
          runAsAdminBtn.disabled = false;
        }
      }
    });
  }

  ["#account-key", "#redeem-key", "#replacement-key"].forEach((id) => {
    $(id)?.addEventListener("change", () => {
      void syncKeys(id);
    });
  });

  // Never restore saved keys/tokens into inputs — leave all fields empty on launch.
  await refreshAdminSettings();
}

boot();
