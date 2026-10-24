// ---------------------------------------------------------------------------
// motion.js — the site's motion primitives, hand-written.
//
// The visual vocabulary is borrowed from the motion-primitives / Watermelon UI
// school of animated interfaces: in-view reveals, staggered groups, letter
// reveal, text scramble, animated counters, pointer-tracked spotlight. What is
// NOT borrowed is the stack (React, Framer Motion, Tailwind): this site runs
// on GitHub Pages with no build step, so the primitives are delivered as one
// dependency-free file of CSS hooks + this observer. That is the whole story
// this project tells about its own dependencies, so it is the exact story the
// site can tell about them.
//
// Progressive-enhancement rules this file enforces everywhere:
//   - Nothing is ever hidden by CSS until this script runs and sets the
//     `js-motion` class. A visitor with JS off sees the full page.
//   - Everything yields to prefers-reduced-motion: the arming code never
//     applies, so no hidden state and no animation is left behind. The nav
//     hairline runs for them anyway — they lose easing, not affordance.
//   - Reveals are one-shot CSS animations, not transitions, so they never
//     hijack properties that own other states (a card's hover transition, for
//     instance) and never re-trigger on the way back up the page.
//   - Counters run FROM THE VALUE IN THE MARKUP and land on that exact value.
//     The markup value is the contract; the animation is only how it arrives.
// ---------------------------------------------------------------------------

(function () {
  'use strict';

  var reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var hasIO = 'IntersectionObserver' in window;

  // --- nav: the hairline appears only once content is underneath it ----------
  // For every visitor, motion policy or not.
  var nav = document.getElementById('nav');
  function navStuck() {
    if (nav) nav.dataset.stuck = String(window.scrollY > 8);
  }
  navStuck();
  window.addEventListener('scroll', navStuck, { passive: true });
  window.addEventListener('pageshow', function (e) {
    if (e.persisted) navStuck();
  });

  if (reduce || !hasIO) return;
  document.documentElement.classList.add('js-motion');

  // One observer for every armed element; unobserve on arrival. One-shot
  // entrances only — repeated animation is what makes pages feel cheap;
  // entrances are what make them feel deliberate.
  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (entry) {
      if (!entry.isIntersecting) return;
      var el = entry.target;
      el.dataset.mpShown = 'true';
      io.unobserve(el);
      if (el.dataset.mpPend === 'scramble') scramble(el);
      if (el.dataset.mpPend === 'count') countUp(el);
    });
  }, { rootMargin: '0px 0px -8% 0px' });

  // arm(el, variant, delay): variant null means "trigger only" — counters and
  // scrambles sit inside already-revealing parents and must not carry a
  // hiding state of their own.
  function arm(el, variant, delay) {
    if (el.dataset.mpArmed) return;
    el.dataset.mpArmed = 'true';
    if (variant) el.dataset.mpVariant = variant;
    if (delay) el.style.setProperty('--mp-delay', delay);
    io.observe(el);
  }

  // --- in-view reveal ---------------------------------------------------------
  // Authoring: data-reveal="up|blur|fade|scale" in markup.
  document.querySelectorAll('[data-reveal]').forEach(function (el) {
    if (el.dataset.reveal === '') el.dataset.reveal = 'up';
    arm(el, el.dataset.reveal, el.dataset.revealDelay);
  });

  // --- staggered groups -------------------------------------------------------
  // Children of [data-stagger] inherit the group's variant with a per-child
  // delay, so a grid arrives as one gesture, not sixteen independent ones.
  // The delay feeds a keyframe animation-delay, which does not touch the
  // elements' own transitions at all.
  document.querySelectorAll('[data-stagger]').forEach(function (group) {
    var step = Number(group.dataset.stagger) || 60;
    var variant = group.dataset.reveal || 'up';
    Array.prototype.forEach.call(group.children, function (child, i) {
      arm(child, variant, 'calc(var(--mp-order, 0) * ' + step + 'ms)');
      child.style.setProperty('--mp-order', String(i));
    });
  });

  // --- the sections every page is made of -------------------------------------
  // Kept from the previous system: whole sections and the hero's children get
  // one entrance without markup noise. Explicit data-reveal (above) wins
  // wherever a page asks for something specific.
  document.querySelectorAll('.hero__grid > *, .section, .section--tight')
    .forEach(function (el) {
      if (!el.dataset.mpArmed) arm(el, 'up');
    });

  // --- letter reveal ----------------------------------------------------------
  // Wrap each visible character and give it a numbered stagger. Applied in
  // markup to one plain-text span only: splitting an <em> across glyph
  // boundaries breaks the reading of the emphasis, and no entrance is worth
  // that. The original text moves to aria-label so the line announces as one
  // run, not glyph by glyph; copy/paste is untouched.
  document.querySelectorAll('[data-letters]').forEach(function (el) {
    var text = el.textContent;
    el.textContent = '';
    el.setAttribute('aria-label', text);
    var mask = document.createElement('span');
    mask.setAttribute('aria-hidden', 'true');
    for (var i = 0; i < text.length; i++) {
      var ch = text[i];
      var span = document.createElement('span');
      span.className = 'mp-letter';
      span.textContent = ch === ' ' ? '\u00A0' : ch;
      span.style.setProperty('--mp-i', String(i));
      mask.appendChild(span);
    }
    el.appendChild(mask);
    el.dataset.mpVariant = 'letters';
    io.observe(el);
  });

  // --- text scramble ----------------------------------------------------------
  // Mono eyebrow labels only; never body text and never values. One pass:
  // characters resolve left to right, then the real label is restored
  // exactly. Mono type keeps the width from thrashing while it runs.
  var CH = '!<>-_\\/[]{}=+*^?#01';
  function scramble(el) {
    var target = el.textContent;
    var start = performance.now();
    var DUR = 480;
    (function frame(now) {
      var t = Math.min(1, (now - start) / DUR);
      var settle = Math.floor(t * target.length);
      var out = '';
      for (var i = 0; i < target.length; i++) {
        if (target[i] === ' ') { out += ' '; continue; }
        out += i < settle ? target[i] : CH[(Math.random() * CH.length) | 0];
      }
      el.textContent = out;
      if (t < 1) requestAnimationFrame(frame);
      else el.textContent = target;
    })(start);
  }

  document.querySelectorAll('[data-scramble]').forEach(function (el) {
    el.dataset.mpPend = 'scramble';
    arm(el, null);
  });

  // --- animated counter ---------------------------------------------------------
  // Counts to the value printed in the markup and stops there, exact to the
  // last digit the markup publishes. easeOutExpo: fast arrival, no overshoot
  // on numbers — a figure that lands at "3.6899" would be a lie.
  function countUp(el) {
    var final = el.getAttribute('data-count');
    if (final === null) return;
    var decimals = (final.split('.')[1] || '').length;
    var start = performance.now();
    var DUR = 900;
    (function frame(now) {
      var t = Math.min(1, (now - start) / DUR);
      var eased = t === 1 ? 1 : 1 - Math.pow(2, -10 * t);
      el.textContent = (Number(final) * eased).toFixed(decimals);
      if (t < 1) requestAnimationFrame(frame);
      else el.textContent = final;
    })(start);
  }

  document.querySelectorAll('[data-count]').forEach(function (el) {
    el.dataset.mpPend = 'count';
    arm(el, null);
  });

  // --- spotlight cards ----------------------------------------------------------
  // The Watermelon card treatment: a light that follows the pointer across
  // the surface. Pointer-only, so touch and keyboard never get a state they
  // cannot move; CSS paints the light from --mx/--my.
  if (window.matchMedia('(hover: hover) and (pointer: fine)').matches) {
    document.querySelectorAll('[data-spotlight]').forEach(function (card) {
      card.addEventListener('pointermove', function (e) {
        var r = card.getBoundingClientRect();
        card.style.setProperty('--mx', (e.clientX - r.left) + 'px');
        card.style.setProperty('--my', (e.clientY - r.top) + 'px');
      });
      card.addEventListener('pointerleave', function () {
        card.style.removeProperty('--mx');
        card.style.removeProperty('--my');
      });
    });
  }
})();

// --- copy buttons: run for every visitor, motion policy or not ---------------
// Added by script rather than written into every block by hand, so the markup
// stays readable and the behaviour is in one place. No clipboard API under
// file:// on some browsers: fall back to a labelled nudge, which is honest
// given the button says "Copy".
document.addEventListener('DOMContentLoaded', function () {
  document.querySelectorAll('.code-block').forEach(function (block) {
    var pre = block.querySelector('pre');
    if (!pre || block.querySelector('.code-block__copy')) return;
    var btn = document.createElement('button');
    btn.className = 'code-block__copy';
    btn.type = 'button';
    btn.setAttribute('aria-label', 'Copy command to clipboard');
    btn.textContent = 'Copy';
    var label = block.querySelector('.code-block__label');
    if (label) label.classList.add('code-block__label--shifted');
    btn.addEventListener('click', function () {
      navigator.clipboard.writeText(pre.innerText).then(function () {
        btn.dataset.done = 'true';
        btn.textContent = 'Copied';
        window.setTimeout(function () {
          btn.dataset.done = 'false';
          btn.textContent = 'Copy';
        }, 1600);
      }, function () {
        btn.textContent = 'Ctrl+C…';
        window.setTimeout(function () { btn.textContent = 'Copy'; }, 1600);
      });
    });
    block.appendChild(btn);
  });
});
