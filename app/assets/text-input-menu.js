// Shovel text-input context menu.
//
// Installed once at app start (see
// `ui::components::context_menu::ContextMenu`). Suppresses the
// webview's native right-click menu inside <input>, <textarea> and
// contenteditable elements, and renders a small floating menu with
// Cut / Copy / Paste / Select all / Clear.
//
// The menu is a pure-DOM overlay so it does not need a round-trip
// through Dioxus for every right-click. Mutating the value of an
// <input> from the menu dispatches a synthetic 'input' event so the
// Rust-side signal bound to the element stays in sync.
//
// Visual style: reuses `.context-menu` / `.context-menu__item` from
// `app.css`. The z-index override (`.shovel-text-menu`) sits above
// the regular menu in case both happen to be visible for one frame.

(function () {
    if (window.__shovelTextInputMenuInstalled) {
        return;
    }
    window.__shovelTextInputMenuInstalled = true;

    var TEXT_INPUT_MENU_ID = 'shovel-text-input-menu';
    var activeMenu = null;
    var closeListeners = null;

    function dismissTextMenu() {
        if (activeMenu) {
            activeMenu.remove();
            activeMenu = null;
        }
        if (closeListeners) {
            document.removeEventListener('mousedown', closeListeners.outside, true);
            document.removeEventListener('contextmenu', closeListeners.outsideMenu, true);
            document.removeEventListener('scroll', closeListeners.scroll, true);
            window.removeEventListener('resize', closeListeners.resize, true);
            window.removeEventListener('blur', closeListeners.scroll, true);
            window.removeEventListener('keydown', closeListeners.esc, true);
            closeListeners = null;
        }
    }

    // -----------------------------------------------------------------
    // Global Escape handler — closes both this text-input menu and
    // the Rust-side `.context-menu-backdrop` overlay. The Dioxus
    // menu does not have a direct JS API, so we dispatch a click on
    // the backdrop just like the original handler did.
    // -----------------------------------------------------------------
    function closeRustMenu() {
        var backdrop = document.querySelector('.context-menu-backdrop');
        if (backdrop) {
            backdrop.click();
        }
    }

    function isTextEditable(node) {
        // Walk up the DOM tree. Some layouts put a transparent
        // `pointer-events: none` overlay on top of the real input
        // (Shovel's SQL editor does this for syntax highlighting),
        // so `event.target` is the overlay element rather than the
        // textarea. We trust the ancestor walk to find the real
        // editable element.
        var el = node;
        while (el && el.nodeType === 1) {
            // The SQL editor has its own Dioxus context menu (format /
            // run / explain). Skip it so the capture listener does not
            // steal the event and paint a theme-less overlay on body.
            if (el.id === 'workspace-sql-editor' ||
                (el.classList && el.classList.contains('sql-editor__input'))) {
                return null;
            }
            var tag = el.tagName;
            if (tag === 'TEXTAREA' && !el.disabled && !el.readOnly) {
                return { kind: 'textarea', el: el };
            }
            if (tag === 'INPUT') {
                var t = (el.type || 'text').toLowerCase();
                // `password` is intentionally NOT in this list.
                // See the comment near the allowed-list below.
                var allowed = ['text', 'search', 'url', 'email', 'tel', 'number', ''];
                if (allowed.indexOf(t) >= 0 && !el.disabled && !el.readOnly) {
                    return { kind: 'input', el: el };
                }
            }
            if (el.isContentEditable) {
                return { kind: 'ce', el: el };
            }
            el = el.parentElement;
        }
        return null;
    }

    function getSelectionInfo(target) {
        try {
            if (target.kind === 'ce') {
                var sel = window.getSelection();
                var text = sel ? String(sel.toString()) : '';
                return { text: text, hasSelection: text.length > 0 };
            }
            var el = target.el;
            var start = el.selectionStart || 0;
            var end = el.selectionEnd || 0;
            var selText = start !== end ? el.value.slice(start, end) : '';
            return { text: selText, hasSelection: selText.length > 0, start: start, end: end };
        } catch (e) {
            return { text: '', hasSelection: false };
        }
    }

    function fireInputEvent(el) {
        // Dioxus 0.7 binds inputs to signals by listening for the
        // standard 'input' event. Without this dispatch the
        // Rust-side value would lag behind any mutation we make
        // from this menu.
        try {
            el.dispatchEvent(new Event('input', { bubbles: true }));
        } catch (e) {
            /* ignore */
        }
    }

    function tryClipboardRead() {
        if (navigator.clipboard && typeof navigator.clipboard.readText === 'function') {
            return navigator.clipboard.readText();
        }
        return Promise.reject(new Error('clipboard API unavailable'));
    }

    function tryClipboardWrite(text) {
        if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
            return navigator.clipboard.writeText(text);
        }
        return Promise.reject(new Error('clipboard API unavailable'));
    }

    function performCut(target) {
        // execCommand is deprecated but still the only cross-context
        // way to remove the selected text from an input. If it
        // fails, we fall back to clearing the value at the selection
        // bounds and dispatching the input event.
        try {
            document.execCommand('cut');
            return;
        } catch (e) {
            /* fallthrough */
        }
        if (target.kind === 'ce') {
            try {
                document.execCommand('insertText', false, '');
            } catch (e) { /* ignore */ }
            return;
        }
        var el = target.el;
        var start = el.selectionStart || 0;
        var end = el.selectionEnd || 0;
        el.value = el.value.slice(0, start) + el.value.slice(end);
        el.selectionStart = el.selectionEnd = start;
        fireInputEvent(el);
    }

    function performCopy(target, text) {
        // Try the modern clipboard API first; fall back to the
        // legacy execCommand if the webview denies permission.
        tryClipboardWrite(text).catch(function () {
            try {
                document.execCommand('copy');
            } catch (e) { /* ignore */ }
        });
    }

    function performPaste(target) {
        tryClipboardRead().then(function (text) {
            if (target.kind === 'ce') {
                try {
                    document.execCommand('insertText', false, text);
                } catch (e) { /* ignore */ }
                return;
            }
            var el = target.el;
            var start = el.selectionStart || 0;
            var end = el.selectionEnd || 0;
            el.value = el.value.slice(0, start) + text + el.value.slice(end);
            el.selectionStart = el.selectionEnd = start + text.length;
            fireInputEvent(el);
        }).catch(function () {
            try {
                document.execCommand('paste');
            } catch (e) { /* ignore */ }
        });
    }

    function performSelectAll(target) {
        try {
            if (target.kind === 'ce') {
                var range = document.createRange();
                range.selectNodeContents(target.el);
                var sel = window.getSelection();
                if (sel) {
                    sel.removeAllRanges();
                    sel.addRange(range);
                }
                return;
            }
            target.el.select();
        } catch (e) { /* ignore */ }
    }

    function performClear(target) {
        if (target.kind === 'ce') {
            try {
                target.el.innerText = '';
                fireInputEvent(target.el);
            } catch (e) { /* ignore */ }
            return;
        }
        target.el.value = '';
        fireInputEvent(target.el);
    }

    function buildMenu(target, x, y) {
        dismissTextMenu();
        var info = getSelectionInfo(target);
        var isReadOnly = !!target.el.readOnly;
        var menu = document.createElement('div');
        menu.id = TEXT_INPUT_MENU_ID;
        menu.className = 'context-menu shovel-text-menu';
        menu.style.left = x + 'px';
        menu.style.top = y + 'px';

        var items = [
            {
                label: 'Cut',
                enabled: info.hasSelection && !isReadOnly,
                run: function () { performCut(target); }
            },
            {
                label: 'Copy',
                enabled: info.hasSelection,
                run: function () { performCopy(target, info.text); }
            },
            {
                label: 'Paste',
                enabled: !isReadOnly,
                run: function () { performPaste(target); }
            },
            { separator: true },
            {
                label: 'Select all',
                enabled: true,
                run: function () { performSelectAll(target); }
            },
            {
                label: 'Clear',
                enabled: !isReadOnly,
                danger: true,
                run: function () { performClear(target); }
            }
        ];

        for (var i = 0; i < items.length; i++) {
            var it = items[i];
            if (it.separator) {
                var sep = document.createElement('div');
                sep.className = 'context-menu__separator';
                menu.appendChild(sep);
                continue;
            }
            var btn = document.createElement('button');
            btn.type = 'button';
            var cls = 'context-menu__item';
            if (it.danger) cls += ' context-menu__item--danger';
            if (!it.enabled) {
                cls += ' context-menu__item--disabled';
                btn.disabled = true;
            }
            btn.className = cls;
            // Split the label on the last `\t\t` boundary so
            // `Cut\t\tCtrl+X` renders as a label + shortcut hint
            // (matches the Rust-side `ContextMenu` rendering).
            var lastSep = it.label.lastIndexOf('\t\t');
            var visible = lastSep >= 0 ? it.label.substring(0, lastSep) : it.label;
            var hint = lastSep >= 0 ? it.label.substring(lastSep + 2) : null;
            var label = document.createElement('span');
            label.className = 'context-menu__item-label';
            label.textContent = visible;
            btn.appendChild(label);
            if (hint) {
                var shortcutEl = document.createElement('span');
                shortcutEl.className = 'context-menu__item-shortcut';
                shortcutEl.textContent = hint;
                btn.appendChild(shortcutEl);
            }
            (function (item) {
                btn.addEventListener('click', function (ev) {
                    ev.stopPropagation();
                    if (!item.enabled) return;
                    item.run();
                    dismissTextMenu();
                });
            })(it);
            menu.appendChild(btn);
        }

        var host = document.querySelector('.app') || document.body;
        host.appendChild(menu);
        activeMenu = menu;

        // Clamp to viewport on open. We use getBoundingClientRect on
        // the just-inserted element to measure the real dimensions.
        var rect = menu.getBoundingClientRect();
        var vw = window.innerWidth;
        var vh = window.innerHeight;
        var pad = 4;
        var left = Math.max(pad, Math.min(x, vw - rect.width - pad));
        var top = Math.max(pad, Math.min(y, vh - rect.height - pad));
        menu.style.left = left + 'px';
        menu.style.top = top + 'px';

        // Wire up the close listeners. Deferred via setTimeout so
        // the current contextmenu event that opened us does not
        // immediately close the menu.
        function closeIfOutside(ev) {
            if (activeMenu && !activeMenu.contains(ev.target)) {
                dismissTextMenu();
            }
        }
        // Scroll events on inner scroll containers (e.g. the
        // workspace sidebar, the explorer, the agent composer) do
        // not fire on `window`, so listening on `window` would
        // miss the common case. Listening on `document` with
        // capture catches scroll on any descendant, since scroll
        // events bubble up the DOM tree.
        function closeOnScroll() {
            dismissTextMenu();
        }
        // Same logic for window resize: the menu is positioned
        // with absolute pixels, so a resize would leave it
        // dangling over the wrong region.
        function closeOnResize() {
            dismissTextMenu();
        }
        function closeOnEsc(ev) {
            if (ev.key === 'Escape') {
                // Close both this menu and the Rust-side menu.
                // When both are open the user expects a single
                // Escape to dismiss the topmost, so we dismiss
                // ours and the backdrop at the same time.
                dismissTextMenu();
                closeRustMenu();
            }
        }
        closeListeners = {
            outside: closeIfOutside,
            outsideMenu: closeIfOutside,
            scroll: closeOnScroll,
            resize: closeOnResize,
            esc: closeOnEsc
        };
        setTimeout(function () {
            document.addEventListener('mousedown', closeIfOutside, true);
            document.addEventListener('contextmenu', closeIfOutside, true);
            // capture:true catches scroll events that originate
            // on any descendant element before they are stopped
            // by an ancestor handler.
            document.addEventListener('scroll', closeOnScroll, true);
            window.addEventListener('resize', closeOnResize, true);
            window.addEventListener('blur', closeOnScroll, true);
            window.addEventListener('keydown', closeOnEsc, true);
        }, 0);
    }

    document.addEventListener('contextmenu', function (event) {
        var editable = isTextEditable(event.target);
        if (!editable) {
            return;
        }
        // Suppress the native menu; render ours.
        event.preventDefault();
        event.stopPropagation();
        buildMenu(editable, event.clientX, event.clientY);
    }, true);
})();
