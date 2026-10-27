(function () {
    var tt = document.getElementById('tt');
    function show(e, text) {
      tt.textContent = text;
      tt.style.left = e.clientX + 'px';
      tt.style.top = e.clientY + 'px';
      tt.style.opacity = '1';
    }
    function hide() { tt.style.opacity = '0'; }
    document.querySelectorAll('[data-t]').forEach(function (el) {
      el.addEventListener('mousemove', function (e) { show(e, el.getAttribute('data-t')); });
      el.addEventListener('mouseleave', hide);
      el.setAttribute('tabindex', '0');
      el.setAttribute('role', 'img');
      el.setAttribute('aria-label', el.getAttribute('data-t'));
    });
  })();
