(function () {
  'use strict';

  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  var KEYWORDS = new Set([
    'let', 'mut', 'fn', 'return', 'if', 'else', 'for', 'while', 'loop', 'break', 'continue',
    'use', 'dump', 'trace', 'scan', 'fetch', 'in', 'struct', 'enum', 'impl', 'trait', 'match',
    'try', 'catch', 'raise', 'throw', 'async', 'await', 'spawn', 'type', 'as', 'mod', 'pub'
  ]);
  // Keywords that produce a value (so a following '/' is division, not a regex).
  var VALUE_WORDS = new Set(['true', 'false', 'nil', 'open', 'port', 'banner']);
  var TYPES = new Set([
    'int', 'string', 'bytes', 'bool', 'hex', 'hex8', 'hex16', 'hex32', 'hex64',
    'i8', 'i16', 'i32', 'i64', 'u8', 'u16', 'u32', 'u64', 'f32', 'f64',
    'Option', 'Result', 'Display', 'Debug', 'Iterable', 'Index', 'IndexMut',
    'Some', 'None', 'Ok', 'Err'
  ]);
  var BUILTINS = new Set([
    'fmt', 'len', 'split', 'join', 'contains', 'upper', 'lower', 'trim', 'replace', 'find',
    'starts_with', 'ends_with', 'slice', 'repeat', 'reverse', 'sum', 'min', 'max', 'abs',
    'sqrt', 'pow', 'clamp', 'sort', 'push', 'keys', 'values', 'has', 'get', 'read', 'write',
    'array', 'map', 'print', 'dbg', 'exit', 'sleep', 'now_ms', 'args', 'env_get', 'ord',
    'chr', 'substr', 'md5', 'sha1', 'sha256', 'xor', 'rot13', 'hex_encode', 'hex_decode',
    'base64_encode', 'base64_decode', 'url_encode', 'url_decode', 'to_hex', 'from_hex',
    'dns_lookup', 'reverse_dns', 'scan_ports', 'scan_subdomains', 'net_listen', 'net_accept',
    'net_connect', 'net_local_addr', 'tcp_read', 'tcp_write', 'tcp_read_line', 'tcp_close',
    'thread_join', 'channel', 'chan_send', 'chan_recv', 'regex_new', 'regex_match',
    'regex_is_match', 'regex_find', 'regex_find_all', 'regex_replace', 'html_title',
    'html_select', 'html_links', 'html_images', 'html_forms', 'html_count', 'file_write',
    'file_read', 'file_exists', 'file_size', 'file_list', 'file_delete', 'file_mkdir',
    'file_copy', 'json_parse', 'json_stringify', 'json_get', 'json_path', 'json_keys',
    'json_find_all'
  ]);
  var TWO = ['|>', '=>', '::', '..', '==', '!=', '<=', '>=', '&&', '||', '<<', '>>', '->', '+=', '-=', '*=', '/=', '%='];

  function escapeHTML(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function readString(line, i) {
    // i points at the opening quote
    var j = i + 1;
    while (j < line.length) {
      if (line[j] === '\\' && j + 1 < line.length) { j += 2; continue; }
      if (line[j] === '"') { j++; break; }
      j++;
    }
    return { text: line.slice(i, j), next: j };
  }

  function readChar(line, i) {
    // i points at the opening '
    var j = i + 1;
    if (line[j] === '\\' && line[j + 1] === 'x' && j + 3 < line.length && /[0-9a-fA-F]{2}/.test(line.slice(j + 2, j + 4))) {
      j += 4;
    } else if (line[j] === '\\' && j + 1 < line.length) {
      j += 2;
    } else if (line[j] && line[j] !== "'") {
      j += 1;
    }
    if (line[j] === "'") j++;
    return { text: line.slice(i, j), next: j };
  }

  function readRegex(line, i) {
    // i points at the opening /
    var j = i + 1;
    var inClass = false;
    while (j < line.length) {
      var c = line[j];
      if (c === '\\' && j + 1 < line.length) { j += 2; continue; }
      if (c === '[') { inClass = true; j++; continue; }
      if (c === ']') { inClass = false; j++; continue; }
      if (c === '/' && !inClass) { j++; break; }
      j++;
    }
    if (j > line.length || line[j - 1] !== '/') return null; // unterminated
    while (j < line.length && /[a-z]/i.test(line[j])) j++;
    return { text: line.slice(i, j), next: j };
  }

  function highlightLine(line) {
    var out = '';
    var i = 0;
    var prevCanEnd = false; // can the previous significant token end an expression?

    function emit(cls, text) {
      if (cls) out += '<span class="' + cls + '">' + escapeHTML(text) + '</span>';
      else out += escapeHTML(text);
    }
    function sig(cls, text) {
      if (!cls) return; // whitespace doesn't change context
      if (cls === 'tok-op') {
        prevCanEnd = (text === ')' || text === ']' || text === '}');
      } else if (cls === 'tok-keyword') {
        prevCanEnd = VALUE_WORDS.has(text);
      } else {
        prevCanEnd = true; // number, string, char, regex, type, builtin, ident
      }
    }

    while (i < line.length) {
      var c = line[i];

      // comment
      if (c === '/' && line[i + 1] === '/') { emit('tok-comment', line.slice(i)); break; }

      // string (and f"/b" prefixes)
      if (c === '"') { var s = readString(line, i); emit('tok-string', s.text); i = s.next; sig('tok-string', s.text); continue; }
      if ((c === 'f' || c === 'b') && line[i + 1] === '"') { var s2 = readString(line, i + 1); emit('tok-string', c + s2.text); i = s2.next; sig('tok-string', s2.text); continue; }

      // char literal
      if (c === "'") { var ch = readChar(line, i); emit('tok-char', ch.text); i = ch.next; sig('tok-char', ch.text); continue; }

      // regex literal (only in operand context)
      if (c === '/' && !prevCanEnd) { var r = readRegex(line, i); if (r) { emit('tok-regex', r.text); i = r.next; sig('tok-regex', r.text); continue; } }

      // hex
      if (c === '0' && (line[i + 1] === 'x' || line[i + 1] === 'X')) {
        var j = i + 2; while (j < line.length && /[0-9A-Fa-f]/.test(line[j])) j++;
        var hx = line.slice(i, j); emit('tok-number', hx); i = j; sig('tok-number', hx); continue;
      }

      // number (with type suffix)
      if (/[0-9]/.test(c)) {
        var j = i; while (j < line.length && /[0-9.]/.test(line[j])) j++;
        var suf = line.slice(j).match(/^(f32|f64|i8|i16|i32|i64|u8|u16|u32|u64)/);
        if (suf) j += suf[0].length;
        var num = line.slice(i, j); emit('tok-number', num); i = j; sig('tok-number', num); continue;
      }

      // identifier / keyword / type / builtin
      if (/[a-zA-Z_]/.test(c)) {
        var j = i; while (j < line.length && /[a-zA-Z0-9_]/.test(line[j])) j++;
        var word = line.slice(i, j);
        var cls = null;
        if (KEYWORDS.has(word)) cls = 'tok-keyword';
        else if (TYPES.has(word)) cls = 'tok-type';
        else if (BUILTINS.has(word)) cls = 'tok-builtin';
        emit(cls, word); i = j; sig(cls, word); continue;
      }

      // two-char operators
      var two = line.slice(i, i + 2);
      if (TWO.indexOf(two) !== -1) { emit('tok-op', two); i += 2; sig('tok-op', two); continue; }

      // single-char operators
      if ('+-*/%&|^!~<>=.,:;()[]{}?'.indexOf(c) !== -1) { emit('tok-op', c); i++; sig('tok-op', c); continue; }

      // whitespace
      if (/\s/.test(c)) { var w = line.slice(i).match(/^\s+/)[0]; emit(null, w); i += w.length; continue; }

      // anything else
      emit(null, c); i++;
    }
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
      if (block.querySelector('.copy-btn')) return;

      var btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'copy-btn';
      btn.textContent = 'Copy';
      btn.setAttribute('aria-label', 'Copy code');
      block.appendChild(btn);

      btn.addEventListener('pointerdown', function (e) { e.currentTarget.classList.add('pressed'); });
      btn.addEventListener('click', function () {
        var text = pre.textContent;
        var done = function () {
          btn.textContent = 'Copied';
          btn.classList.add('copied');
          window.setTimeout(function () { btn.textContent = 'Copy'; btn.classList.remove('copied'); }, 1800);
        };
        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(text).then(done, function () { fallbackCopy(text); done(); });
        } else { fallbackCopy(text); done(); }
      });
      btn.addEventListener('pointerup', function () { btn.classList.remove('pressed'); });

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
        if (entry.isIntersecting) { entry.target.classList.add('is-visible'); io.unobserve(entry.target); }
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
      if (reduceMotion) bar.style.width = bar.getAttribute('data-width') + '%';
      else io.observe(bar);
    });
  }

  function initScrollSpy() {
    var sections = Array.prototype.slice.call(document.querySelectorAll('section.doc-section[id]'));
    var links = Array.prototype.slice.call(document.querySelectorAll('.docs-toc a[href^="#"]'));
    var navLinks = Array.prototype.slice.call(document.querySelectorAll('.site-nav a[href^="#"]'));
    var allLinks = links.concat(navLinks);
    if (!sections.length || !allLinks.length) return;

    var activeEls = { toc: links[0], nav: navLinks[0] };
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
        sections.forEach(function (sec) { if (pos >= sec.offsetTop - 140) current = sec.getAttribute('id'); });
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
