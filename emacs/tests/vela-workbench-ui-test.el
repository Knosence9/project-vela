;;; vela-workbench-ui-test.el --- Tests for Vela's optional Emacs UI -*- lexical-binding: t; -*-

(require 'ert)
(require 'vela-agent-mode)
(require 'vela-workbench-ui)

(ert-deftest vela-workbench-ui-applies-a-doom-inspired-interface ()
  (with-temp-buffer
    (vela-agent-interface-mode)
    (setq-local vela-agent-interface-source-buffer
                (get-buffer-create "*vela-ui-source*"))
    (unwind-protect
        (save-window-excursion
          (set-window-buffer (selected-window) (current-buffer))
          (vela-workbench-ui-mode 1)
          (should vela-workbench-ui-mode)
          (should (equal line-spacing 0.12))
          (should (eq cursor-type 'bar))
          (should (string-match-p "VELA" (vela-workbench-ui--header-line)))
          (should (string-match-p "READ ONLY" (prin1-to-string mode-line-format))))
      (kill-buffer "*vela-ui-source*"))))

(ert-deftest vela-workbench-ui-local-enable-is-idempotent-and-restores-bindings ()
  (with-temp-buffer
    (vela-agent-interface-mode)
    (should-not (local-variable-p 'line-spacing))
    (let ((font-lock-keywords-before (copy-tree font-lock-keywords))
          (font-lock-keywords-local-before
           (local-variable-p 'font-lock-keywords)))
      (setq-local cursor-type 'box)
      (vela-workbench-ui-mode 1)
      (vela-workbench-ui-mode 1)
      (vela-workbench-ui-mode -1)
      (should-not (local-variable-p 'line-spacing))
      (should (local-variable-p 'cursor-type))
      (should (eq cursor-type 'box))
      (should (equal font-lock-keywords font-lock-keywords-before))
      (should (eq (local-variable-p 'font-lock-keywords)
                  font-lock-keywords-local-before)))))

(ert-deftest vela-workbench-ui-enable-is-reversible ()
  (let ((initial-menu-bar-mode menu-bar-mode)
        (initial-tool-bar-mode tool-bar-mode))
    (unwind-protect
        (progn
          (menu-bar-mode 1)
          (tool-bar-mode 1)
          (vela-workbench-ui-enable)
          (should (memq 'vela-doom custom-enabled-themes))
          (should (memq #'vela-workbench-ui--enable-managed-buffer
                        vela-agent-interface-mode-hook))
          (should (equal (face-background 'default nil t) "#282c34"))
          (should-not menu-bar-mode)
          (should-not tool-bar-mode)
          (vela-workbench-ui-disable)
          (should-not (memq 'vela-doom custom-enabled-themes))
          (should-not (memq #'vela-workbench-ui--enable-managed-buffer
                            vela-agent-interface-mode-hook))
          (should menu-bar-mode)
          (should tool-bar-mode))
      (vela-workbench-ui-disable)
      (menu-bar-mode (if initial-menu-bar-mode 1 -1))
      (tool-bar-mode (if initial-tool-bar-mode 1 -1)))))

(ert-deftest vela-workbench-ui-enable-preserves-preexisting-presentation ()
  (let ((vela-workbench-ui--saved-global-state nil)
        (vela-workbench-ui--managed-buffers nil))
    (unwind-protect
        (with-temp-buffer
          (vela-agent-interface-mode)
          (vela-workbench-ui-mode 1)
          (enable-theme 'vela-doom)
          (add-hook 'vela-agent-interface-mode-hook #'vela-workbench-ui-mode)
          (vela-workbench-ui-enable)
          (vela-workbench-ui-disable)
          (should vela-workbench-ui-mode)
          (should (memq 'vela-doom custom-enabled-themes))
          (should (memq #'vela-workbench-ui-mode
                        vela-agent-interface-mode-hook)))
      (remove-hook 'vela-agent-interface-mode-hook #'vela-workbench-ui-mode)
      (disable-theme 'vela-doom))))

(ert-deftest vela-workbench-ui-global-hook-ignores-current-local-binding ()
  (let ((default-hook-before
         (copy-sequence (default-value 'vela-agent-interface-mode-hook))))
    (unwind-protect
        (with-temp-buffer
          (setq-local vela-agent-interface-mode-hook nil)
          (vela-workbench-ui-enable)
          (should
           (memq #'vela-workbench-ui--enable-managed-buffer
                 (default-value 'vela-agent-interface-mode-hook)))
          (with-temp-buffer
            (vela-agent-interface-mode)
            (should vela-workbench-ui-mode))
          (vela-workbench-ui-disable)
          (should
           (equal (default-value 'vela-agent-interface-mode-hook)
                  default-hook-before)))
      (vela-workbench-ui-disable)
      (set-default 'vela-agent-interface-mode-hook default-hook-before))))

(ert-deftest vela-workbench-ui-manual-enable-revokes-global-buffer-ownership ()
  (unwind-protect
      (with-temp-buffer
        (vela-agent-interface-mode)
        (vela-workbench-ui-enable)
        (should vela-workbench-ui-mode)
        (should (memq (current-buffer) vela-workbench-ui--managed-buffers))
        (vela-workbench-ui-mode -1)
        (should-not vela-workbench-ui-mode)
        (should (memq (current-buffer) vela-workbench-ui--managed-buffers))
        (vela-workbench-ui-mode 1)
        (should-not (memq (current-buffer) vela-workbench-ui--managed-buffers))
        (vela-workbench-ui-disable)
        (should vela-workbench-ui-mode))
    (vela-workbench-ui-disable)))

(ert-deftest vela-workbench-ui-highlights-structured-protocol-values ()
  (with-temp-buffer
    (insert "{\"operation\\\"name\": \"context\\\"snapshot\", "
            "\"offset\": -1.25e+2, \"ok\": true, "
            "\"values\": [1,2,3,4]}")
    (vela-agent-interface-mode)
    (vela-workbench-ui-mode 1)
    (font-lock-ensure)
    (goto-char (point-min))
    (search-forward "operation")
    (should (eq (get-text-property (match-beginning 0) 'face)
                'vela-workbench-json-key-face))
    (search-forward "-1.25e+2")
    (should (eq (get-text-property (match-beginning 0) 'face)
                'vela-workbench-json-constant-face))
    (search-forward "true")
    (should (eq (get-text-property (match-beginning 0) 'face)
                'vela-workbench-json-constant-face))
    (dolist (number '("1" "2" "3" "4"))
      (search-forward number)
      (should (eq (get-text-property (match-beginning 0) 'face)
                  'vela-workbench-json-constant-face)))))

(provide 'vela-workbench-ui-test)
;;; vela-workbench-ui-test.el ends here
