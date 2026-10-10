// Scroll reveal, platform detection and direct download links from the latest GitHub release.
(function () {
  "use strict";

  var io = "IntersectionObserver" in window
    ? new IntersectionObserver(function (entries) {
        entries.forEach(function (e) {
          if (e.isIntersecting) {
            e.target.classList.add("in");
            io.unobserve(e.target);
          }
        });
      }, { threshold: 0.12 })
    : null;
  document.querySelectorAll(".reveal").forEach(function (el) {
    if (io) io.observe(el); else el.classList.add("in");
  });

  var ua = navigator.userAgent;
  var os = /Mac/i.test(ua) ? "mac" : /Win/i.test(ua) ? "win" : /Linux|X11/i.test(ua) ? "linux" : null;
  var arm = /arm|aarch64/i.test(ua) || (navigator.userAgentData && navigator.userAgentData.platform === "macOS");
  var names = { mac: "macOS", win: "Windows", linux: "Linux" };
  if (os) {
    var card = document.querySelector('.platform[data-os="' + os + '"]');
    if (card) {
      card.classList.add("recommended");
      card.querySelector(".os").textContent = "Recommended for you";
    }
    var hero = document.getElementById("hero-download");
    if (hero) hero.textContent = "Download for " + names[os];
  }

  function pick(assets, test) {
    for (var i = 0; i < assets.length; i++) if (test(assets[i].name)) return assets[i].browser_download_url;
    return null;
  }

  fetch("https://api.github.com/repos/devops-monk/photo-studio/releases/latest", { headers: { Accept: "application/vnd.github+json" } })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (rel) {
      if (!rel || !rel.assets) return;
      var a = rel.assets;
      var links = {
        mac: pick(a, function (n) { return /macos.*\.dmg$/.test(n); }),
        win: pick(a, function (n) { return new RegExp("windows-" + (arm ? "arm64" : "x64") + "\\.zip$").test(n); })
          || pick(a, function (n) { return /windows.*\.zip$/.test(n); }),
        linux: pick(a, function (n) { return /linux-x86_64\.AppImage$/.test(n); }) || pick(a, function (n) { return /linux.*\.tar\.gz$/.test(n); }),
      };
      Object.keys(links).forEach(function (k) {
        var el = document.querySelector('.platform[data-os="' + k + '"]');
        if (el && links[k]) el.href = links[k];
      });
      var line = document.getElementById("version-line");
      if (line && rel.tag_name) line.textContent = "Version " + rel.tag_name.replace(/^v/, "") + " · Free and open source under the MIT license.";
      var hero = document.getElementById("hero-download");
      if (hero && os && links[os]) hero.href = links[os];
    })
    .catch(function () {});
})();
