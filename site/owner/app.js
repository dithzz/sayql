// SayQL owner portal. Talks only to /api/owner; the session lives in an HttpOnly cookie.
// Everything shown is set with textContent, never as HTML.
(function () {
  "use strict";
  var $ = function (id) { return document.getElementById(id); };
  var DAY = 86400000;

  function api(action, method, body) {
    var opts = { method: method || "GET", credentials: "same-origin", headers: {} };
    if (body !== undefined) {
      opts.headers["Content-Type"] = "application/json";
      opts.body = JSON.stringify(body);
    }
    if (opts.method === "POST") opts.headers["X-Requested-With"] = "sayql-owner";
    return fetch("/api/owner?action=" + action, opts).then(function (r) {
      return r.json().catch(function () { return {}; }).then(function (j) {
        if (!r.ok) { var e = new Error(j.error || "Request failed (" + r.status + ")"); e.status = r.status; throw e; }
        return j;
      });
    });
  }

  function el(tag, text, cls) {
    var n = document.createElement(tag);
    if (text != null) n.textContent = String(text);
    if (cls) n.className = cls;
    return n;
  }
  function show(id, on) { $(id).hidden = !on; }
  function err(id, msg) { $(id).textContent = msg || ""; show(id, !!msg); }
  function date(s) { return s ? String(s).slice(0, 10) : "—"; }
  function money(n) { return "$" + Math.round(n).toLocaleString("en-US"); }
  // Cents, or four decimals for amounts under a cent so small usage still shows.
  function usd(micro) {
    var v = (micro || 0) / 1e6, a = Math.abs(v);
    return (v < 0 ? "−$" : "$") + a.toFixed(a > 0 && a < 0.01 ? 4 : 2);
  }
  function within(s, days) { return s && Date.now() - Date.parse(s) <= days * DAY; }
  function today() { return new Date().toISOString().slice(0, 10); }

  function rows(id, items, cells, empty) {
    var body = $(id);
    body.textContent = "";
    if (!items.length) {
      var tr = el("tr"), td = el("td", empty, "subtle");
      td.colSpan = 10; tr.append(td); body.append(tr); return;
    }
    items.forEach(function (it) {
      var tr = el("tr");
      cells(it).forEach(function (c) {
        if (c instanceof Node) { var td = el("td"); td.append(c); tr.append(td); }
        else if (c && typeof c === "object") tr.append(el("td", c.text, c.cls));
        else tr.append(el("td", c == null || c === "" ? "—" : c));
      });
      body.append(tr);
    });
  }

  function tile(n, label, sub) {
    var t = el("div", null, "card tile");
    t.append(el("div", n, "n"), el("div", label, "l"));
    if (sub) t.append(el("div", sub, "s"));
    return t;
  }

  function gate(label, have, need) {
    var g = el("div", null, "gate"), bar = el("div", null, "meter"), fill = el("span");
    fill.style.width = Math.min(100, Math.round((have / need) * 100)) + "%";
    bar.append(fill);
    g.append(el("div", label), bar, el("div", have + " of " + need, "small muted"));
    return g;
  }

  function render(d) {
    var now = today();
    var active = d.licences.filter(function (l) { return l.expires >= now; });
    var paying = active.filter(function (l) { return l.plan === "team"; });
    var mrr = paying.reduce(function (s, l) { return s + (Number(l.price_usd_month) || 0); }, 0);
    var soon = active.filter(function (l) { return Date.parse(l.expires) - Date.now() <= 30 * DAY; });
    var installs30 = d.installs.filter(function (r) { return within(r.day, 30); }).reduce(function (s, r) { return s + r.runs; }, 0);
    var downloads = Array.isArray(d.downloads) ? d.downloads : [];
    var dlTotal = downloads.reduce(function (s, r) { return s + r.macOS + r.Windows + r.Linux; }, 0);
    var partners = d.licences.filter(function (l) { return /partner/i.test(l.notes || ""); }).length;

    var tiles = $("tiles");
    tiles.textContent = "";
    tiles.append(
      tile(money(mrr), "Monthly recurring revenue", paying.length + " paying " + (paying.length === 1 ? "team" : "teams")),
      tile(active.length, "Active licences", soon.length ? soon.length + " expire within 30 days" : "none expiring soon"),
      tile(d.trials.length, "Trial sign-ups", d.trials.filter(function (t) { return within(t.created_at, 7); }).length + " this week"),
      tile(d.waitlist.length, "Waitlist", d.waitlist.filter(function (w) { return within(w.created_at, 7); }).length + " this week"),
      tile(installs30, "Installer runs", "last 30 days"),
      tile(dlTotal, "App downloads", "all releases")
    );

    var gates = $("gates");
    gates.textContent = "";
    gates.append(
      gate("Waitlist sign-ups", d.waitlist.length, 100),
      gate("Signed design partners", partners, 3),
      el("p", "Partners count licences whose notes say “partner”. The third test (5 people say yes to $149 a month) comes from your calls.", "small subtle")
    );

    rows("licences", d.licences, function (l) {
      var cls = l.expires < now ? "bad" : Date.parse(l.expires) - Date.now() <= 30 * DAY ? "warn" : "good";
      var pill = el("span", l.expires, "pill " + cls);
      return [l.licensee, l.contact_email, l.plan, { text: l.users, cls: "num" }, l.issued, pill,
        { text: l.price_usd_month == null ? "—" : money(l.price_usd_month), cls: "num" }, l.notes];
    }, "No licences recorded yet. Record each key you sign below.");

    var ai = d.ai || [];
    rows("ai", ai, function (a) {
      var m = a.month || { requests: 0, tokens: 0, charged_micro: 0, cost_micro: 0, last_used: null };
      var bal = el("span", usd(a.balance_micro), "pill " + (!a.active ? "" : a.balance_micro <= 0 ? "bad" : a.balance_micro < 5e6 ? "warn" : "good"));
      var toggle = el("button", a.active ? "Turn off" : "Turn on", a.active ? "danger" : "");
      toggle.type = "button";
      toggle.addEventListener("click", function () {
        if (a.active && !confirm("Turn off " + a.name + "? Their key stops working at once.")) return;
        api("ai-active", "POST", { account_id: a.id, active: !a.active }).then(load, function (e) { err("error", e.message); });
      });
      var rekey = el("button", "New key");
      rekey.type = "button";
      rekey.addEventListener("click", function () {
        if (!confirm("Give " + a.name + " a new key? Their current key stops working at once; the balance and history stay.")) return;
        api("ai-rekey", "POST", { account_id: a.id }).then(function (r) { showKey(r.key); return load(); },
          function (e) { err("error", e.message); });
      });
      var actions = el("span", null, "row-actions");
      actions.append(rekey, toggle);
      return [a.name + (a.active ? "" : " (off)"), a.contact_email, bal, { text: m.requests, cls: "num" },
        { text: m.tokens.toLocaleString("en-US"), cls: "num" }, { text: usd(m.charged_micro), cls: "num" },
        { text: usd(m.cost_micro), cls: "num" }, date(m.last_used), actions];
    }, "No credit accounts yet. Create one below for a team that wants SayQL to provide the AI.");
    var sel = $("ai-topup-account");
    var keep = sel.value;
    sel.textContent = "";
    ai.forEach(function (a) { var o = el("option", a.name + " (" + usd(a.balance_micro) + ")"); o.value = a.id; sel.append(o); });
    if (keep) sel.value = keep;

    rows("trials", d.trials, function (t) {
      return [date(t.created_at), t.company, t.email, t.size, t.server_version];
    }, "No trial sign-ups yet.");

    rows("waitlist", d.waitlist, function (w) {
      return [date(w.created_at), w.email, { team: "Team", gateway: "Agent gateway", both: "Both" }[w.interest] || w.interest, w.size];
    }, "No sign-ups yet.");

    var chart = $("installs");
    chart.textContent = "";
    var byDay = {};
    d.installs.forEach(function (r) { byDay[r.day] = r.runs; });
    var max = Math.max.apply(null, [1].concat(d.installs.map(function (r) { return r.runs; })));
    for (var i = 89; i >= 0; i--) {
      var day = new Date(Date.now() - i * DAY).toISOString().slice(0, 10);
      var bar = el("span");
      bar.style.height = Math.max(2, Math.round(((byDay[day] || 0) / max) * 64)) + "px";
      if (!byDay[day]) bar.className = "none";
      bar.title = day + ": " + (byDay[day] || 0);
      chart.append(bar);
    }
    var total90 = d.installs.reduce(function (s, r) { return s + r.runs; }, 0);
    $("installs-note").textContent = total90 + " in 90 days. Counted per day only: no IP or anything else. Includes re-runs to update.";

    if (Array.isArray(d.downloads)) {
      rows("downloads", downloads, function (r) {
        return [r.tag, date(r.published_at), { text: r.macOS, cls: "num" }, { text: r.Windows, cls: "num" },
          { text: r.Linux, cls: "num" }, { text: r.macOS + r.Windows + r.Linux, cls: "num" }];
      }, "No releases.");
    } else {
      rows("downloads", [], null, "GitHub didn't answer: " + (d.downloads && d.downloads.error));
    }

    $("updated").textContent = "Updated " + new Date(d.generated_at).toLocaleTimeString();
  }

  function load() {
    show("loading", true);
    err("error", "");
    return api("data").then(function (d) {
      show("loading", false); show("signin-view", false); show("app", true); show("signout", true);
      render(d);
    }, function (e) {
      show("loading", false);
      if (e.status === 401) { show("app", false); show("signout", false); show("signin-view", true); }
      else err("error", e.message);
    });
  }

  $("signin").addEventListener("submit", function (ev) {
    ev.preventDefault();
    err("signin-error", "");
    var pw = ev.target.password.value;
    api("login", "POST", { password: pw }).then(function () { ev.target.reset(); return load(); },
      function (e) { err("signin-error", e.message); });
  });

  $("signout").addEventListener("click", function () {
    api("logout", "POST", {}).then(function () { location.reload(); });
  });

  $("ai-new").addEventListener("submit", function (ev) {
    ev.preventDefault();
    var f = ev.target;
    err("ai-new-error", "");
    api("ai-account", "POST", { name: f.name.value, contact_email: f.contact_email.value, initial_usd: f.initial_usd.value })
      .then(function (r) {
        f.reset();
        showKey(r.key);
        return load();
      }, function (e) { err("ai-new-error", e.message); });
  });

  // A key shown this once, from a new account or a new key for an existing one.
  function showKey(key) {
    $("ai-key-value").value = key;
    show("ai-key", true);
    var box = $("ai-key").closest("details");
    if (box) box.open = true;
    $("ai-key").scrollIntoView({ block: "center" });
  }

  $("ai-key-copy").addEventListener("click", function () {
    var b = $("ai-key-copy");
    navigator.clipboard.writeText($("ai-key-value").value).then(function () { b.textContent = "Copied"; },
      function () { $("ai-key-value").select(); b.textContent = "Select it"; });
    setTimeout(function () { b.textContent = "Copy"; }, 1500);
  });

  $("ai-topup").addEventListener("submit", function (ev) {
    ev.preventDefault();
    var f = ev.target;
    err("ai-topup-error", "");
    $("ai-topup-ok").textContent = "";
    api("ai-topup", "POST", { account_id: Number(f.account_id.value), usd: f.usd.value, note: f.note.value, kind: f.adjust.checked ? "adjust" : "topup" })
      .then(function (r) {
        f.usd.value = ""; f.note.value = ""; f.adjust.checked = false;
        $("ai-topup-ok").textContent = "Done. Balance " + usd(r.balance_micro) + ".";
        return load();
      }, function (e) { err("ai-topup-error", e.message); });
  });

  $("licence-form").addEventListener("submit", function (ev) {
    ev.preventDefault();
    var f = ev.target;
    err("licence-error", "");
    $("licence-ok").textContent = "";
    api("licence", "POST", { key: f.key.value, contact_email: f.contact_email.value, price_usd_month: f.price_usd_month.value, notes: f.notes.value })
      .then(function (r) {
        f.reset();
        $("licence-ok").textContent = "Recorded " + r.licence.licensee + " (" + r.licence.plan + ", " + r.licence.users + " seats, until " + r.licence.expires + ").";
        return load();
      }, function (e) { err("licence-error", e.message); });
  });

  load();
})();
