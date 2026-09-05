(function () {
  'use strict';

  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  var KEYWORDS = new RegExp('\\b(let|mut|fn|return|if|else|for|while|loop|break|continue|use|dump|trace|scan|fetch|in|true|false|nil|open|port|banner)\\b', 'g');

  var BUILTINS = new RegExp('\\b(fmt|len|split|join|contains|upper|lower|trim|md5|sha1|sha256|xor_encrypt|rot13|dns_lookup|reverse_dns|scan_ports|scan_subdomains|html_title|html_select|html_links|html_images|html_forms|html_count|file_write|file_read|file_exists|file_size|file_list|file_delete|file_mkdir|file_copy|json_get|json_path|json_keys|json_find_all|hex_encode|hex_decode|base64_encode|url_encode|to_hex|from_hex)\\b', 'g');

  function escapeHTML(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function highlightLine(line) {
    var out = '';
    var re = /(\/\/.*$)|("(?:\\.|[^"\\])*")|(\b0x[0-9A-Fa-f]+\b)|(\b\d+\b)|(\b[a-zA-Z_][a-zA-Z0-9_]*\b)|(\s+)|(.)/g;
    var m;
    var last = 0;

    while ((m = re.exec(line)) !== null) {
      if (m.index > last) out += line.slice(last, m.index);
      last = re.lastIndex;

      if (m[1]) out += '<span class="tok-comment">' + escapeHTML(m[1]) + '</span>';
      else if (m[2]) out += '<span class="tok-string">' + escapeHTML(m[2]) + '</span>';
      else if (m[3] || m[4]) out += '<span class="tok-number">' + escapeHTML(m[3] || m[4]) + '</span>';
      else if (m[5]) {
        var word = m[5];
        var cls = null;
        if (KEYWORDS.test(word)) { cls = 'tok-keyword'; KEYWORDS.lastIndex = 0; }
        else if (BUILTINS.test(word)) { cls = 'tok-builtin'; BUILTINS.lastIndex = 0; }
        out += cls ? '<span class="' + cls + '">' + word + '</span>' : word;
      }
      else if (m[6]) out += m[6];
      else if (m[7]) out += escapeHTML(m[7]);

      if (m[0] === '') re.lastIndex++;
    }

    if (last < line.length) out += line.slice(last);
    return out;
  }

  function highlightBlocks() {
    document.querySelectorAll('code[data-lang="rak"]').forEach(function (code) {
      if (code.dataset.highlighted) return;
      var lines = code.textContent.split('\n');
      code.innerHTML = lines.map(highlightLine).join('\n');
      code.dataset.highlighted = '1';
    });
  }

  function initCopyButtons() {
    document.querySelectorAll('.codeblock').forEach(function (block) {
      var pre = block.querySelector('pre');
      if (!pre) return;

      var btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'copy-btn';
      btn.textContent = 'Copy';
      btn.setAttribute('aria-label', 'Copy code');
      block.appendChild(btn);

      btn.addEventListener('pointerdown', function (e) {
        e.currentTarget.classList.add('pressed');
      });

      btn.addEventListener('click', function () {
        var text = pre.textContent;
        var done = function () {
          btn.textContent = 'Copied';
          btn.classList.add('copied');
          window.setTimeout(function () {
            btn.textContent = 'Copy';
            btn.classList.remove('copied');
          }, 1800);
        };

        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(text).then(done, function () { fallbackCopy(text); done(); });
        } else {
          fallbackCopy(text);
          done();
        }
      });

      btn.addEventListener('pointerup', function () {
        btn.classList.remove('pressed');
      });

      function fallbackCopy(text) {
        var ta = document.createElement('textarea');
        ta.value = text;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand('copy'); } catch (err) {}
        document.body.removeChild(ta);
      }
    });
  }

  function initReveals() {
    var els = document.querySelectorAll('.reveal');
    if (reduceMotion || !('IntersectionObserver' in window)) {
      els.forEach(function (el) { el.classList.add('is-visible'); });
      return;
    }

    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.classList.add('is-visible');
          io.unobserve(entry.target);
        }
      });
    }, { threshold: 0.12, rootMargin: '0px 0px -8% 0px' });

    els.forEach(function (el) { io.observe(el); });
  }

  function initProgressBars() {
    var bars = document.querySelectorAll('.progress span[data-width]');
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.style.width = entry.target.getAttribute('data-width') + '%';
          io.unobserve(entry.target);
        }
      });
    }, { threshold: 0.4 });

    bars.forEach(function (bar) {
      if (reduceMotion) {
        bar.style.width = bar.getAttribute('data-width') + '%';
      } else {
        io.observe(bar);
      }
    });
  }

  function initScrollSpy() {
    var sections = Array.prototype.slice.call(document.querySelectorAll('section.doc-section[id]'));
    var links = Array.prototype.slice.call(document.querySelectorAll('.docs-toc a[href^="#"]'));
    var navLinks = Array.prototype.slice.call(document.querySelectorAll('.site-nav a[href^="#"]'));
    var allLinks = links.concat(navLinks);
    if (!sections.length || !allLinks.length) return;

    var activeEls = {
      toc: links[0],
      nav: navLinks[0]
    };

    function setActive(id) {
      if (activeEls.toc) {
        activeEls.toc.classList.remove('active');
        activeEls.toc = links.filter(function (l) { return l.getAttribute('href') === '#' + id; })[0] || null;
        if (activeEls.toc) activeEls.toc.classList.add('active');
      }
      if (activeEls.nav) {
        activeEls.nav.classList.remove('active');
        activeEls.nav = navLinks.filter(function (l) { return l.getAttribute('href') === '#' + id; })[0] || null;
        if (activeEls.nav) activeEls.nav.classList.add('active');
      }
    }

    var ticking = false;
    window.addEventListener('scroll', function () {
      if (ticking) return;
      ticking = true;
      window.requestAnimationFrame(function () {
        var pos = window.scrollY;
        var current = sections[0].getAttribute('id');
        sections.forEach(function (sec) {
          if (pos >= sec.offsetTop - 140) current = sec.getAttribute('id');
        });
        setActive(current);
        ticking = false;
      });
    }, { passive: true });

    allLinks.forEach(function (link) {
      link.addEventListener('click', function (e) {
        if (reduceMotion) return;
        var id = link.getAttribute('href');
        if (!id || id.charAt(0) !== '#') return;
        var target = document.querySelector(id);
        if (!target) return;
        e.preventDefault();
        var y = target.getBoundingClientRect().top + window.scrollY - 84;
        window.scrollTo({ top: y, behavior: 'smooth' });
        history.replaceState(null, '', id);
      });
    });
  }

  document.addEventListener('DOMContentLoaded', function () {
    highlightBlocks();
    initCopyButtons();
    initReveals();
    initProgressBars();
    initScrollSpy();
  });
})();
