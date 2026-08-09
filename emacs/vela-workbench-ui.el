;;; vela-workbench-ui.el --- Doom-inspired UI for Vela's Emacs workbench -*- lexical-binding: t; -*-

;; Copyright (C) 2026 Project Vela contributors
;; SPDX-License-Identifier: MIT
;; Package-Requires: ((emacs "30.1"))
;; Keywords: faces, tools, convenience

;;; Commentary:

;; This optional module gives `vela-agent-interface-mode' a dependency-light,
;; Doom-inspired presentation.  It changes presentation only: the typed agent
;; protocol, editor authority boundary, and read-only behavior remain in
;; `vela-agent-mode'.

;;; Code:

(require 'vela-agent-mode)
(require 'font-lock)

(defgroup vela-workbench-ui nil
  "Presentation for Vela's model-neutral Emacs workbench."
  :group 'vela-agent)

(deftheme vela-doom
  "A dependency-light Doom-inspired dark theme for the Vela workbench.")

(custom-theme-set-faces
 'vela-doom
 '(default ((t (:background "#282c34" :foreground "#bbc2cf"
                :family "DejaVu Sans Mono" :height 120))))
 '(cursor ((t (:background "#51afef"))))
 '(fringe ((t (:background "#282c34" :foreground "#5b6268"))))
 '(region ((t (:background "#3e4451" :extend t))))
 '(highlight ((t (:background "#3e4451"))))
 '(hl-line ((t (:background "#2c323c" :extend t))))
 '(minibuffer-prompt ((t (:foreground "#51afef" :weight bold))))
 '(mode-line ((t (:background "#1b2229" :foreground "#bbc2cf"
                  :box nil :height 0.95))))
 '(mode-line-inactive ((t (:background "#21242b" :foreground "#5b6268"
                           :box nil :height 0.95))))
 '(header-line ((t (:background "#21242b" :foreground "#bbc2cf"
                   :box nil :height 1.05))))
 '(font-lock-builtin-face ((t (:foreground "#c678dd"))))
 '(font-lock-comment-face ((t (:foreground "#5b6268" :slant italic))))
 '(font-lock-constant-face ((t (:foreground "#a9a1e1"))))
 '(font-lock-function-name-face ((t (:foreground "#51afef" :weight semi-bold))))
 '(font-lock-keyword-face ((t (:foreground "#c678dd" :weight semi-bold))))
 '(font-lock-string-face ((t (:foreground "#98be65"))))
 '(font-lock-type-face ((t (:foreground "#ECBE7B"))))
 '(font-lock-variable-name-face ((t (:foreground "#dcaeea"))))
 '(font-lock-warning-face ((t (:foreground "#ff6c6b" :weight bold))))
 '(link ((t (:foreground "#51afef" :underline t))))
 '(org-level-1 ((t (:foreground "#51afef" :weight bold :height 1.25))))
 '(org-level-2 ((t (:foreground "#c678dd" :weight bold :height 1.15))))
 '(org-level-3 ((t (:foreground "#98be65" :weight semi-bold :height 1.08))))
 '(org-block ((t (:background "#21242b" :extend t))))
 '(org-block-begin-line ((t (:background "#21242b" :foreground "#5b6268"
                            :extend t))))
 '(org-block-end-line ((t (:background "#21242b" :foreground "#5b6268"
                          :extend t))))
 '(org-todo ((t (:foreground "#ff6c6b" :weight bold))))
 '(org-done ((t (:foreground "#98be65" :weight bold))))
 '(show-paren-match ((t (:background "#51afef" :foreground "#282c34"
                        :weight bold)))))

(provide-theme 'vela-doom)

(defface vela-workbench-accent-face
  '((t :foreground "#51afef" :weight bold))
  "Accent face for Vela workbench identity.")

(defface vela-workbench-muted-face
  '((t :foreground "#5b6268"))
  "Muted face for secondary workbench information.")

(defface vela-workbench-state-face
  '((t :foreground "#282c34" :background "#98be65" :weight bold))
  "Face for the read-only state badge.")

(defface vela-workbench-json-key-face
  '((t :foreground "#c678dd"))
  "Face for JSON object keys in the interface buffer.")

(defface vela-workbench-json-string-face
  '((t :foreground "#98be65"))
  "Face for JSON string values in the interface buffer.")

(defface vela-workbench-json-constant-face
  '((t :foreground "#da8548" :weight semi-bold))
  "Face for JSON constants and numbers in the interface buffer.")

(defconst vela-workbench-ui--font-lock-keywords
  '(("\"\\(?:\\\\.\\|[^\"\\\\\n]\\)*\""
     0 'vela-workbench-json-string-face t)
    ("\\(\"\\(?:\\\\.\\|[^\"\\\\\n]\\)*\"\\)[[:space:]]*:"
     1 'vela-workbench-json-key-face t)
    ("\\_<\\(?:true\\|false\\|null\\)\\_>"
     . 'vela-workbench-json-constant-face)
    ("\\(?:\\`\\|[][,:{][[:space:]]*\\)\\(-?\\(?:0\\|[1-9][0-9]*\\)\\(?:\\.[0-9]+\\)?\\(?:[eE][+-]?[0-9]+\\)?\\)\\(?:[[:space:]]*[]},]\\|\\'\\)"
     1 'vela-workbench-json-constant-face))
  "Additional font-lock rules for the structured interface response.")

(defconst vela-workbench-ui--local-variables
  '(header-line-format mode-line-format line-spacing cursor-type truncate-lines)
  "Buffer-local presentation variables managed by the workbench UI.")

(defvar-local vela-workbench-ui--saved-local-state nil
  "Values and binding state saved by the local workbench UI mode.")

(defvar vela-workbench-ui--saved-global-state nil
  "Presentation ownership saved while the global Vela UI is enabled.")

(defvar vela-workbench-ui--managed-buffers nil
  "Buffers whose local UI was enabled by `vela-workbench-ui-enable'.")

(defun vela-workbench-ui--source-name ()
  "Return the current interface source buffer name for display."
  (if (buffer-live-p vela-agent-interface-source-buffer)
      (buffer-name vela-agent-interface-source-buffer)
    "no source buffer"))

(defun vela-workbench-ui--header-line ()
  "Return the compact Vela workbench header line."
  (concat
   (propertize "  VELA " 'face 'vela-workbench-accent-face)
   (propertize "AGENT WORKBENCH" 'face 'vela-workbench-muted-face)
   (propertize "  /  " 'face 'vela-workbench-muted-face)
   (propertize (vela-workbench-ui--source-name)
               'face 'mode-line-buffer-id)))

(defun vela-workbench-ui--mode-line ()
  "Return the compact Vela workbench mode line."
  (list
   (propertize " VELA " 'face 'vela-workbench-accent-face)
   "  "
   (propertize " READ ONLY " 'face 'vela-workbench-state-face)
   "  "
   '(:eval (propertize (vela-workbench-ui--source-name)
                       'face 'mode-line-buffer-id))
   '(:eval (propertize
            (format "   L%d:C%d  " (line-number-at-pos) (current-column))
            'face 'vela-workbench-muted-face))))

(defun vela-workbench-ui--snapshot-local-state ()
  "Snapshot managed variables, including whether each binding is local."
  (mapcar (lambda (variable)
            (list variable (local-variable-p variable) (symbol-value variable)))
          vela-workbench-ui--local-variables))

(defun vela-workbench-ui--restore-local-state ()
  "Restore managed variables and their original binding semantics."
  (dolist (entry vela-workbench-ui--saved-local-state)
    (let ((variable (nth 0 entry))
          (local (nth 1 entry))
          (value (nth 2 entry)))
      (if local
          (set (make-local-variable variable) value)
        (kill-local-variable variable))))
  (setq vela-workbench-ui--saved-local-state nil))

;;;###autoload
(define-minor-mode vela-workbench-ui-mode
  "Present the Vela agent interface with a Doom-inspired local UI."
  :init-value nil
  :lighter nil
  (if vela-workbench-ui-mode
      (unless vela-workbench-ui--saved-local-state
        (setq vela-workbench-ui--saved-local-state
              (vela-workbench-ui--snapshot-local-state))
        (setq-local header-line-format '(:eval (vela-workbench-ui--header-line)))
        (setq-local mode-line-format (vela-workbench-ui--mode-line))
        (setq-local line-spacing 0.12)
        (setq-local cursor-type 'bar)
        (setq-local truncate-lines nil)
        (font-lock-add-keywords nil vela-workbench-ui--font-lock-keywords 'append)
        (font-lock-flush))
    (when vela-workbench-ui--saved-local-state
      (font-lock-remove-keywords nil vela-workbench-ui--font-lock-keywords)
      (vela-workbench-ui--restore-local-state)
      (font-lock-flush))))

(defun vela-workbench-ui--enable-managed-buffer ()
  "Enable the local UI and remember that the global command owns it."
  (unless vela-workbench-ui-mode
    (vela-workbench-ui-mode 1)
    (push (current-buffer) vela-workbench-ui--managed-buffers)))

;;;###autoload
(defun vela-workbench-ui-enable ()
  "Enable the Doom-inspired theme and presentation for Vela interfaces."
  (interactive)
  (unless vela-workbench-ui--saved-global-state
    (let ((theme-owned (not (memq 'vela-doom custom-enabled-themes)))
          (hook-owned (not (memq #'vela-workbench-ui--enable-managed-buffer
                                 vela-agent-interface-mode-hook))))
      (setq vela-workbench-ui--saved-global-state
            (list :menu-bar menu-bar-mode
                  :tool-bar tool-bar-mode
                  :theme-owned theme-owned
                  :hook-owned hook-owned)))
    (menu-bar-mode -1)
    (tool-bar-mode -1)
    (enable-theme 'vela-doom)
    (add-hook 'vela-agent-interface-mode-hook
              #'vela-workbench-ui--enable-managed-buffer)
    (dolist (buffer (buffer-list))
      (with-current-buffer buffer
        (when (derived-mode-p 'vela-agent-interface-mode)
          (vela-workbench-ui--enable-managed-buffer))))))

;;;###autoload
(defun vela-workbench-ui-disable ()
  "Disable the optional Vela workbench presentation and theme."
  (interactive)
  (when vela-workbench-ui--saved-global-state
    (when (plist-get vela-workbench-ui--saved-global-state :hook-owned)
      (remove-hook 'vela-agent-interface-mode-hook
                   #'vela-workbench-ui--enable-managed-buffer))
    (dolist (buffer vela-workbench-ui--managed-buffers)
      (when (buffer-live-p buffer)
        (with-current-buffer buffer
          (when vela-workbench-ui-mode
            (vela-workbench-ui-mode -1)))))
    (when (and (plist-get vela-workbench-ui--saved-global-state :theme-owned)
               (memq 'vela-doom custom-enabled-themes))
      (disable-theme 'vela-doom))
    (menu-bar-mode
     (if (plist-get vela-workbench-ui--saved-global-state :menu-bar) 1 -1))
    (tool-bar-mode
     (if (plist-get vela-workbench-ui--saved-global-state :tool-bar) 1 -1))
    (setq vela-workbench-ui--managed-buffers nil
          vela-workbench-ui--saved-global-state nil)))

(provide 'vela-workbench-ui)
;;; vela-workbench-ui.el ends here
