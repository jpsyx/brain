    const toast = document.getElementById('toast');
    let toastTimer = null;
    function showToast(msg) {
      toast.textContent = msg;
      toast.classList.add('visible');
      if (toastTimer) clearTimeout(toastTimer);
      toastTimer = setTimeout(() => toast.classList.remove('visible'), 2200);
    }

    const completedSection = document.getElementById('completed-section');
    const toggleBtn = document.getElementById('completed-toggle');
    const ctText = toggleBtn ? toggleBtn.querySelector('.ct-text') : null;
    const completedGrid = document.getElementById('completed-grid');
    const completedCountEl = document.getElementById('completed-count');

    const ACCORDION_KEY = 'habits:completed-open';
    function applyOpenState(open) {
      completedSection.classList.toggle('open', open);
      if (toggleBtn) {
        toggleBtn.setAttribute('aria-expanded', String(open));
        if (ctText) {
          ctText.textContent = open ? 'Hide completed habits' : 'Show completed habits';
        }
      }
    }

    // Restore state without animating on first paint.
    completedSection.classList.add('no-transition');
    if (toggleBtn) toggleBtn.classList.add('no-transition');
    applyOpenState(localStorage.getItem(ACCORDION_KEY) === '1');
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        completedSection.classList.remove('no-transition');
        if (toggleBtn) toggleBtn.classList.remove('no-transition');
      });
    });

    function toggleCompleted() {
      const open = !completedSection.classList.contains('open');
      applyOpenState(open);
      try { localStorage.setItem(ACCORDION_KEY, open ? '1' : '0'); } catch (_) {}
    }
    if (toggleBtn) toggleBtn.addEventListener('click', toggleCompleted);

    // React-Query-style refetch on focus: when the tab regains visibility
    // after being hidden, just reload — the server re-renders fresh and the
    // response is small + uncached, so this is cheap.
    let hasBeenHidden = false;
    function maybeRefresh() {
      if (document.visibilityState === 'visible' && hasBeenHidden) {
        location.reload();
      }
    }
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'hidden') hasBeenHidden = true;
      else maybeRefresh();
    });
    window.addEventListener('focus', maybeRefresh);
    window.addEventListener('blur', () => { hasBeenHidden = true; });

    function escapeHtml(s) {
      return String(s)
        .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
    }

    function buildCompletedCard(name, duration, notes) {
      const card = document.createElement('article');
      card.className = 'card card--completed entering';
      if (notes) card.title = notes;
      const chips = [];
      if (duration) chips.push('<span class="meta-chip">' + escapeHtml(duration) + 'm</span>');
      chips.push('<span class="meta-chip meta-done">✓ done</span>');
      card.innerHTML =
        '<div class="pri-bar pri-bar--muted"></div>' +
        '<div class="card-body">' +
          '<div class="card-title">' + escapeHtml(name) + '</div>' +
          '<div class="meta-row">' + chips.join('') + '</div>' +
        '</div>';
      return card;
    }

    function refreshPendingCount() {
      const remaining = document.querySelectorAll('#habit-list .card').length;
      const countEl = document.getElementById('count');
      if (countEl) countEl.textContent = remaining + ' to do';
      document.querySelectorAll('.time-section').forEach(sec => {
        if (sec.querySelectorAll('.card').length === 0) sec.remove();
      });
      document.querySelectorAll('.pri-section').forEach(sec => {
        if (sec.querySelectorAll('.card').length === 0) sec.remove();
      });
      if (remaining === 0) {
        document.getElementById('habit-list').innerHTML =
          '<div class="empty">All habits done for today. Nice work.</div>';
      }
    }

    function bumpCompletedCount() {
      const n = completedGrid.querySelectorAll('.card').length;
      if (completedCountEl) completedCountEl.textContent = String(n);
      // Reveal the header toggle once the first habit is completed today.
      if (toggleBtn && n > 0 && toggleBtn.style.display === 'none') {
        toggleBtn.style.display = '';
      }
    }

    function moveToCompleted(card) {
      const name = card.querySelector('.card-title').textContent;
      const durChip = card.querySelector('.meta-chip');
      const duration = durChip && /^\d+m$/.test(durChip.textContent)
        ? durChip.textContent.replace('m', '')
        : '';
      const notes = card.getAttribute('title') || '';
      const newCard = buildCompletedCard(name, duration, notes);
      completedGrid.appendChild(newCard);
      // flush + drop .entering to trigger the fade-in transition
      requestAnimationFrame(() => {
        requestAnimationFrame(() => newCard.classList.remove('entering'));
      });
      bumpCompletedCount();
    }

    document.querySelectorAll('.done-btn').forEach((btn) => {
      btn.addEventListener('click', async () => {
        const taskId = btn.dataset.taskId;
        const card = btn.closest('.card');
        const name = card.querySelector('.card-title').textContent;
        btn.disabled = true;
        try {
          const res = await fetch('{{HABITS_DONE_URL}}', {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({task_id: taskId}),
          });
          if (!res.ok) {
            const err = await res.text();
            throw new Error(err || ('HTTP ' + res.status));
          }
          const data = await res.json();
          card.classList.add('dismissed');
          const nextMsg = data.next_due ? ' · next ' + data.next_due : '';
          showToast('Done: ' + name + nextMsg);
          setTimeout(() => {
            moveToCompleted(card);
            card.remove();
            refreshPendingCount();
          }, 280);
        } catch (err) {
          btn.disabled = false;
          showToast('Failed: ' + err.message);
        }
      });
    });
