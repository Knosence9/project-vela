;;; vela-agent-mode.el --- Model-neutral Emacs interface for Vela -*- lexical-binding: t; -*-

;; Copyright (C) 2026 Project Vela contributors
;; SPDX-License-Identifier: MIT
;; Package-Requires: ((emacs "30.1"))
;; Keywords: tools, convenience

;;; Commentary:

;; This package exposes a deliberately small, read-only interface that lets an
;; agent discover Emacs capabilities and inspect editor context without driving
;; the UI or evaluating arbitrary Emacs Lisp.  Expensive work belongs in
;; external asynchronous workers; Emacs handlers should only snapshot native
;; editor state on the main thread.

;;; Code:

(require 'org)
(require 'ob-core)
(require 'json)
(require 'cl-lib)

(define-error 'vela-agent-protocol-error "Invalid Vela agent request")

(defconst vela-agent-protocol-version 3
  "Version of the model-neutral Vela Emacs protocol.")

(defconst vela-agent-max-buffer-characters (* 1024 1024)
  "Largest buffer accepted by a synchronous context snapshot.")

(defconst vela-agent-max-request-fields 8
  "Largest number of fields accepted in one typed request object.")

(defconst vela-agent-max-request-key-characters 64
  "Largest request object key accepted by the protocol.")

(defconst vela-agent-max-operation-characters 64
  "Largest operation or context-section name accepted by the protocol.")

(defconst vela-agent-max-metadata-string-characters 8192
  "Largest live editor metadata string accepted by a context snapshot.")

(defconst vela-agent-max-json-string-characters 8192
  "Largest string accepted by the deterministic JSON encoder.")

(defconst vela-agent-max-json-collection-items 128
  "Largest object or array accepted by the deterministic JSON encoder.")

(defconst vela-agent-max-json-depth 16
  "Largest nesting depth accepted by the deterministic JSON encoder.")

(defconst vela-agent-max-json-nodes 512
  "Largest value-node count accepted by the deterministic JSON encoder.")

(defconst vela-agent-max-json-output-characters (* 256 1024)
  "Largest encoded response returned by the deterministic JSON encoder.")

(defvar vela-agent--editor-thread (current-thread)
  "Emacs thread that owns Vela interface access to live editor state.")

(defvar-local vela-agent-interface-source-buffer nil
  "Buffer whose context is rendered by this interface buffer.")

(defvar-keymap vela-agent-interface-mode-map
  :doc "Keymap for `vela-agent-interface-mode'."
  "g" #'vela-agent-interface-refresh
  "q" #'quit-window)

(define-derived-mode vela-agent-interface-mode special-mode "Vela-Agent"
  "Human-readable view of the same structured context exposed to agents.")

(defun vela-agent--capabilities ()
  "Return stable read-only operations supported by this package."
  (vector
   '(("name" . "capabilities.list")
     ("effect" . "read"))
   '(("name" . "context.snapshot")
     ("effect" . "read"))))

(defun vela-agent--emacs-feature (name function agent-use context-section)
  "Describe NAME using callable FUNCTION for AGENT-USE and CONTEXT-SECTION."
  `(("name" . ,name)
    ("available" . ,(vela-agent--boolean (fboundp function)))
    ("threading" . "main-thread-snapshot")
    ("context_section" . ,(vela-agent--nullable context-section))
    ("agent_use" . ,agent-use)))

(defun vela-agent--emacs-features ()
  "Return the stable Emacs power-user feature catalog."
  (vector
   `(("name" . "buffer")
     ("available" . t)
     ("threading" . "main-thread-snapshot")
     ("context_section" . "buffer")
     ("agent_use" . "read-only buffer identity, position, mode, and region metadata"))
   (vela-agent--emacs-feature
    "org" 'org-mode "read-only heading, stable ID, and source-block metadata" "org")
   (vela-agent--emacs-feature
    "project" 'project-current "loaded project.el facility metadata" nil)
   (vela-agent--emacs-feature
    "diagnostics" 'flymake-diagnostics "loaded Flymake diagnostics facility metadata" nil)
   (vela-agent--emacs-feature
    "compilation" 'compilation-start "loaded compilation-mode facility metadata" nil)
   (vela-agent--emacs-feature
    "magit" 'magit-status "loaded Magit facility metadata" nil)))

(defun vela-agent--success (operation result)
  "Return a successful envelope for OPERATION containing RESULT."
  `(("protocol_version" . ,vela-agent-protocol-version)
    ("ok" . t)
    ("operation" . ,operation)
    ("result" . ,result)))

(defun vela-agent--nullable (value)
  "Return VALUE, or the JSON null marker when VALUE is nil."
  (if value value :null))

(defun vela-agent--boolean (value)
  "Return VALUE as a JSON-compatible boolean."
  (if value t :false))

(defun vela-agent--bounded-metadata-string (value)
  "Return editor metadata VALUE when it satisfies the protocol bound."
  (unless (and (stringp value)
               (<= (length value) vela-agent-max-metadata-string-characters))
    (signal 'vela-agent-protocol-error
            '("editor metadata exceeds the synchronous response bound")))
  value)

(defun vela-agent--bounded-nullable-metadata-string (value)
  "Return bounded metadata VALUE, or the JSON null marker when VALUE is nil."
  (if value (vela-agent--bounded-metadata-string value) :null))

(defun vela-agent--bounded-metadata-vector (values)
  "Return proper string list VALUES as a bounded protocol vector."
  (let ((cursor values)
        (count 0)
        items)
    (while (consp cursor)
      (when (>= count vela-agent-max-json-collection-items)
        (signal 'vela-agent-protocol-error
                '("editor metadata exceeds the collection bound")))
      (push (vela-agent--bounded-metadata-string (car cursor)) items)
      (setq cursor (cdr cursor)
            count (1+ count)))
    (unless (null cursor)
      (signal 'vela-agent-protocol-error
              '("editor metadata must be a proper string list")))
    (vconcat (nreverse items))))

(defun vela-agent--buffer-context ()
  "Snapshot bounded metadata for the current buffer without moving point."
  `(("name" . ,(vela-agent--bounded-metadata-string (buffer-name)))
    ("file" . ,(if buffer-file-name
                    (vela-agent--bounded-metadata-string buffer-file-name)
                  :null))
    ("major_mode" . ,(vela-agent--bounded-metadata-string
                       (symbol-name major-mode)))
    ("modified" . ,(vela-agent--boolean (buffer-modified-p)))
    ("point" . ,(point))
    ("line" . ,(line-number-at-pos))
    ("column" . ,(current-column))
    ("region" .
              ,(if (use-region-p)
                   `(("start" . ,(region-beginning))
                     ("end" . ,(region-end)))
                 :null))
    ("text_revision" . ,(buffer-chars-modified-tick))
    ("restriction" . (("start" . ,(point-min))
                       ("end" . ,(point-max))
                       ("narrowed" . ,(vela-agent--boolean
                                        (buffer-narrowed-p)))))))

(defun vela-agent--org-heading-context ()
  "Return native Org heading metadata at point, or JSON null."
  (save-excursion
    (condition-case err
        (progn
          (org-back-to-heading t)
          `(("id" . ,(vela-agent--bounded-nullable-metadata-string
                       (org-entry-get nil "ID")))
            ("title" . ,(vela-agent--bounded-metadata-string
                          (org-get-heading t t t t)))
            ("level" . ,(org-current-level))
            ("todo" . ,(vela-agent--bounded-nullable-metadata-string
                         (org-get-todo-state)))
            ("tags" . ,(vela-agent--bounded-metadata-vector
                         (org-get-tags nil t)))
            ("outline_path" . ,(vela-agent--bounded-metadata-vector
                                 (org-get-outline-path t t)))))
      (vela-agent-protocol-error
       (signal (car err) (cdr err)))
      (error :null))))

(defun vela-agent--org-source-block-context ()
  "Return native Org Babel source-block metadata at point, or JSON null."
  (let ((info (org-babel-get-src-block-info 'light)))
    (if info
        `(("name" . ,(vela-agent--bounded-nullable-metadata-string
                       (nth 4 info)))
          ("language" . ,(vela-agent--bounded-metadata-string (car info)))
          ("source_sha256" . ,(secure-hash 'sha256 (nth 1 info))))
      :null)))

(defun vela-agent--org-context ()
  "Snapshot Org metadata at point using native Org APIs."
  (save-match-data
    (if (derived-mode-p 'org-mode)
        `(("heading" . ,(vela-agent--org-heading-context))
          ("source_block" . ,(vela-agent--org-source-block-context)))
      :null)))

(defun vela-agent--context-snapshot (request)
  "Return only the explicitly requested context sections from REQUEST."
  (let ((include (alist-get "include" request nil nil #'string=)))
    (unless (vectorp include)
      (signal 'vela-agent-protocol-error
              '("context.snapshot requires an include vector")))
    (when (> (length include) 2)
      (signal 'vela-agent-protocol-error
              '("context.snapshot accepts at most two sections")))
    (let ((sections (append include nil)))
      (when (and (= (length sections) 2)
                 (equal (car sections) (cadr sections)))
        (signal 'vela-agent-protocol-error
                '("context.snapshot sections must be unique")))
      (dolist (section sections)
        (unless (and (stringp section)
                     (<= (length section) vela-agent-max-operation-characters)
                     (member section '("buffer" "org")))
          (signal 'vela-agent-protocol-error
                  '("unsupported context section"))))
      (when (> (buffer-size) vela-agent-max-buffer-characters)
        (signal 'vela-agent-protocol-error
                (list (format "buffer exceeds context snapshot limit: %d characters"
                              vela-agent-max-buffer-characters))))
      (let (result)
        (when (member "buffer" sections)
          (push (cons "buffer" (vela-agent--buffer-context)) result))
        (when (member "org" sections)
          (push (cons "org" (vela-agent--org-context)) result))
        (nreverse result)))))

(defun vela-agent--validate-request-object (request)
  "Validate REQUEST as a small, finite, unambiguous string-keyed alist."
  (let ((cursor request)
        (fields 0)
        keys)
    (while (consp cursor)
      (when (>= fields vela-agent-max-request-fields)
        (signal 'vela-agent-protocol-error
                '("agent request has too many fields")))
      (let* ((entry (car cursor))
             (key (and (consp entry) (car entry))))
        (unless (and (stringp key)
                     (<= (length key) vela-agent-max-request-key-characters))
          (signal 'vela-agent-protocol-error
                  '("agent request keys must be bounded strings")))
        (when (member key keys)
          (signal 'vela-agent-protocol-error
                  '("agent request keys must be unique")))
        (push key keys))
      (setq fields (1+ fields)
            cursor (cdr cursor)))
    (unless (null cursor)
      (signal 'vela-agent-protocol-error
              '("agent request must be a proper object")))
    request))

(defun vela-agent-handle-request (request)
  "Handle one typed, read-only REQUEST and return JSON-compatible data."
  (unless (eq (current-thread) vela-agent--editor-thread)
    (signal 'vela-agent-protocol-error
            '("agent requests must run on the editor owner thread")))
  (vela-agent--validate-request-object request)
  (let ((operation (alist-get "operation" request nil nil #'string=)))
    (unless (and (stringp operation)
                 (<= (length operation) vela-agent-max-operation-characters))
      (signal 'vela-agent-protocol-error
              '("agent request requires a bounded operation name")))
    (pcase operation
      ("capabilities.list"
       (vela-agent--success
        operation
        `(("capabilities" . ,(vela-agent--capabilities))
          ("emacs_features" . ,(vela-agent--emacs-features)))))
      ("context.snapshot"
       (vela-agent--success operation (vela-agent--context-snapshot request)))
      (_
       (signal 'vela-agent-protocol-error
               '("unsupported operation"))))))

(defun vela-agent--json-serialize (value depth active node-count)
  "Serialize VALUE at DEPTH with ACTIVE ancestors and bounded NODE-COUNT."
  (when (> depth vela-agent-max-json-depth)
    (signal 'vela-agent-protocol-error
            '("JSON response exceeds the nesting-depth bound")))
  (aset node-count 0 (1+ (aref node-count 0)))
  (when (> (aref node-count 0) vela-agent-max-json-nodes)
    (signal 'vela-agent-protocol-error
            '("JSON response exceeds the value-node bound")))
  (cond
   ((eq value :null) "null")
   ((eq value :false) "false")
   ((eq value t) "true")
   ((stringp value)
    (when (> (length value) vela-agent-max-json-string-characters)
      (signal 'vela-agent-protocol-error
              '("JSON string exceeds the response bound")))
    (json-serialize value))
   ((numberp value)
    (let ((encoded (json-serialize value)))
      (when (> (length encoded) vela-agent-max-json-string-characters)
        (signal 'vela-agent-protocol-error
                '("JSON number exceeds the response bound")))
      encoded))
   ((vectorp value)
    (when (> (length value) vela-agent-max-json-collection-items)
      (signal 'vela-agent-protocol-error
              '("JSON array exceeds the collection bound")))
    (when (gethash value active)
      (signal 'vela-agent-protocol-error '("cyclic JSON response")))
    (puthash value t active)
    (unwind-protect
        (let (items)
          (dotimes (index (length value))
            (push (vela-agent--json-serialize
                   (aref value index) (1+ depth) active node-count)
                  items))
          (concat "[" (mapconcat #'identity (nreverse items) ",") "]"))
      (remhash value active)))
   ((listp value)
    (when (gethash value active)
      (signal 'vela-agent-protocol-error '("cyclic JSON response")))
    (puthash value t active)
    (unwind-protect
        (let ((cursor value)
              (count 0)
              items)
          (while (consp cursor)
            (when (>= count vela-agent-max-json-collection-items)
              (signal 'vela-agent-protocol-error
                      '("JSON object exceeds the collection bound")))
            (let ((entry (car cursor)))
              (unless (and (consp entry)
                           (stringp (car entry))
                           (<= (length (car entry))
                               vela-agent-max-json-string-characters))
                (signal 'vela-agent-protocol-error
                        '("JSON object has an invalid bounded key")))
              (push (concat
                     (json-serialize (car entry)) ":"
                     (vela-agent--json-serialize
                      (cdr entry) (1+ depth) active node-count))
                    items))
            (setq cursor (cdr cursor)
                  count (1+ count)))
          (unless (null cursor)
            (signal 'vela-agent-protocol-error
                    '("JSON object must be a proper list")))
          (concat "{" (mapconcat #'identity (nreverse items) ",") "}"))
      (remhash value active)))
   (t
    (signal 'vela-agent-protocol-error
            '("value is not JSON-compatible")))))

(defun vela-agent-encode-response (response)
  "Encode typed RESPONSE as deterministic JSON for an external transport."
  (let ((encoded
         (vela-agent--json-serialize
          response 0 (make-hash-table :test #'eq) (vector 0))))
    (when (> (length encoded) vela-agent-max-json-output-characters)
      (signal 'vela-agent-protocol-error
              '("encoded JSON response exceeds the output bound")))
    encoded))

(defun vela-agent-interface-refresh ()
  "Refresh the interface from `vela-agent-interface-source-buffer'."
  (interactive)
  (unless (buffer-live-p vela-agent-interface-source-buffer)
    (user-error "The Vela agent source buffer is no longer live"))
  (let ((response
         (with-current-buffer vela-agent-interface-source-buffer
           (vela-agent-handle-request
            '(("operation" . "context.snapshot")
              ("include" . ["buffer" "org"]))))))
    (let ((inhibit-read-only t))
      (erase-buffer)
      (insert (vela-agent-encode-response response))
      (json-pretty-print-buffer)
      (goto-char (point-min)))))

;;;###autoload
(defun vela-agent-interface-open ()
  "Open the read-only Vela agent interface for the current buffer."
  (interactive)
  (let ((source (current-buffer))
        (interface (get-buffer-create "*Vela Agent Interface*")))
    (with-current-buffer interface
      (vela-agent-interface-mode)
      (setq vela-agent-interface-source-buffer source)
      (vela-agent-interface-refresh))
    (display-buffer interface)
    interface))

(provide 'vela-agent-mode)

;;; vela-agent-mode.el ends here
