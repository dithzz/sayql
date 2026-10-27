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
