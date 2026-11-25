// SPDX-License-Identifier: AGPL-3.0-or-later
// Static web flasher: zero build step, works from file:// for everything except WebSerial.
(function () {
  "use strict";

  var state = { boards: [], release: null, webSerial: false };

  function $(selector) { return document.querySelector(selector); }

  function escapeHtml(value) {
    return String(value == null ? "" : value)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function releaseUrl(board, fileName) {
    if (!state.release || !state.release.base_url) return "#";
    return state.release.base_url.replace(/\/$/, "") + "/" + fileName;
  }

  function detectSupport() {
    var banner = $("#support-banner");
    var secure = window.isSecureContext || location.hostname === "localhost" || location.hostname === "127.0.0.1";
    state.webSerial = !!navigator.serial && secure;
    var parts = [];
    var cls = "banner show ";
    if (state.webSerial) {
      parts.push("WebSerial is available — you can flash the ESP32-S3 tracker directly from this page.");
      cls += "good";
    } else if (navigator.serial && !secure) {
      parts.push("WebSerial needs a secure context: open this page over https:// or http://localhost instead of file://.");
      cls += "warn";
    } else if (location.protocol === "file:") {
      parts.push("Opened from file://: board cards, UF2 downloads and documentation work, but in-browser ESP32 flashing needs http(s) + Chrome/Edge.");
      cls += "warn";
    } else {
      parts.push("This browser has no WebSerial support. Use Chrome/Edge 89+ for in-browser ESP32 flashing, or the Linux CLI below for any board.");
      cls += "warn";
    }
    var nrf = state.boards.filter(function (b) { return b.flash === "uf2"; }).length;
    if (nrf > 0) parts.push(nrf + " nRF52 board(s) flash by copying a UF2 file — works in every browser and OS.");
    banner.className = cls;
    banner.textContent = parts.join(" ");
  }

  function renderCard(board) {
    var isWeb = board.flash === "web-serial";
    var role = board.role_class === "repeater" ? "repeater" : "tracker";
    var card = document.createElement("article");
    card.className = "board-card";
    card.dataset.role = role;

    var features = (board.features || [])
      .map(function (f) { return "<li>" + escapeHtml(f) + "</li>"; })
      .join("");

    var actions = "";
    if (isWeb) {
      var disabled = state.webSerial ? "" : " disabled";
      actions =
        '<esp-web-install-button manifest="' + escapeHtml(board.manifest) + '"' + disabled + ">" +
        '<button slot="activate" class="btn btn-primary" type="button">Flash from this browser</button>' +
        '<span slot="unsupported">WebSerial is not supported in this browser — use the Linux CLI.</span>' +
        '<span slot="not-allowed">Open this page over http(s) to use WebSerial.</span>' +
        "</esp-web-install-button>" +
        '<p class="card-note">esp-web-tools downloads the merged <code>.bin</code> and writes it over WebSerial. Chrome or Edge required.</p>';
    } else {
      var uf2 = releaseUrl(board, board.uf2);
      actions =
        '<a class="btn btn-primary" href="' + escapeHtml(uf2) + '" download>Download ' + escapeHtml(board.uf2) + "</a>" +
        '<a class="btn btn-secondary" href="' + escapeHtml(board.manifest || "#linux") + '" hidden></a>' +
        '<ol class="steps">' +
        "<li>Double-tap reset (or double-tap the side button) to enter the UF2 bootloader.</li>" +
        "<li>A USB drive appears (e.g. <code>T1000E</code>, <code>RAK4631</code>, <code>SEEEDSOLAR</code>).</li>" +
        "<li>Copy the downloaded <code>.uf2</code> file onto that drive. The board reboots by itself.</li>" +
        "</ol>" +
        '<p class="card-note">' + escapeHtml(board.notes) + "</p>" +
        '<a class="btn btn-ghost" href="' + escapeHtml(uf2) + '.sha256">SHA-256 checksum</a>';
    }

    card.innerHTML =
      '<div class="card-top">' +
        "<div><h3>" + escapeHtml(board.name) + "</h3>" +
        '<p class="tagline">' + escapeHtml(board.tagline) + "</p></div>" +
        '<span class="chip">' + escapeHtml(board.role) + "</span>" +
      "</div>" +
      '<dl class="specs">' +
        "<div><dt>MCU</dt><dd>" + escapeHtml(board.mcu) + "</dd></div>" +
        "<div><dt>Radio</dt><dd>" + escapeHtml(board.radio) + "</dd></div>" +
        "<div><dt>GNSS</dt><dd>" + escapeHtml(board.gnss) + "</dd></div>" +
        "<div><dt>Flash</dt><dd>" + escapeHtml(board.flash_mb) + " MB</dd></div>" +
        "<div><dt>Power</dt><dd>" + escapeHtml(board.battery) + "</dd></div>" +
        "<div><dt>Antenna</dt><dd>" + escapeHtml(board.antenna) + "</dd></div>" +
      "</dl>" +
      '<ul class="feature-list">' + features + "</ul>" +
      '<div class="card-actions">' + actions + "</div>";
    return card;
  }

  function render() {
    var grid = $("#board-grid");
    grid.innerHTML = "";
    state.boards.forEach(function (board) { grid.appendChild(renderCard(board)); });
    grid.setAttribute("aria-busy", "false");
    if (state.release) {
      $("#release-line").textContent =
        "Firmware release " + state.release.tag + " · artefacts: " + state.release.base_url;
    }
  }

  function fallbackData() {
    return window.OPENMESH_BOARDS || { boards: [], release: null };
  }

  function initCopyButtons() {
    document.querySelectorAll(".copy[data-copy]").forEach(function (button) {
      button.addEventListener("click", function () {
        var target = document.getElementById(button.dataset.copy);
        if (!target) return;
        var text = target.textContent || "";
        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(text).then(function () {
            button.textContent = "Copied!";
            setTimeout(function () { button.textContent = "Copy"; }, 1500);
          });
        }
      });
    });
  }

  function boot() {
    var data = fallbackData();
    state.boards = data.boards || [];
    state.release = data.release || null;
    // Prefer boards.json when served over http so releases can be updated without a rebuild.
    if (location.protocol !== "file:" && window.fetch) {
      fetch("boards.json", { cache: "no-cache" })
        .then(function (response) { return response.ok ? response.json() : null; })
        .then(function (json) {
          if (json && json.boards) {
            state.boards = json.boards;
            state.release = json.release;
          }
        })
        .catch(function () { /* offline fallback already applied */ })
        .finally(function () { detectSupport(); render(); });
    } else {
      detectSupport();
      render();
    }
    initCopyButtons();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
