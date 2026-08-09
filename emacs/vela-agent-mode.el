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
(require 'project)
(require 'flymake)

(define-error 'vela-agent-protocol-error "Invalid Vela agent request")

(defconst vela-agent-protocol-version 8
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

(defconst vela-agent-max-diagnostics-json-characters (* 128 1024)
  "Largest aggregate encoded diagnostic collection accepted by a snapshot.")

(defconst vela-agent-max-compilation-count (* 1024 1024)
  "Largest native compilation diagnostic count accepted by a snapshot.")

(defconst vela-agent-max-json-string-characters 8192
  "Largest string accepted by the deterministic JSON encoder.")

(defconst vela-agent-max-json-collection-items 128
  "Largest object or array accepted by the deterministic JSON encoder.")

(defconst vela-agent-max-json-depth 16
  "Largest nesting depth accepted by the deterministic JSON encoder.")

(defconst vela-agent-max-json-nodes
  (+ 5                         ; success envelope through its result object
     17                        ; complete buffer section
     268                       ; complete Org section
     2                         ; complete project section
     1                         ; diagnostics vector
     (* vela-agent-max-json-collection-items 5)
     5                         ; complete compilation section
     2)                        ; complete Magit section
  "Largest value-node count accepted by the deterministic JSON encoder.

This admits one complete six-section snapshot with the maximum collection of
four-field diagnostics while retaining a finite traversal bound.")

(defconst vela-agent-max-json-output-characters (* 256 1024)
  "Largest encoded response returned by the deterministic JSON encoder.")

(defconst vela-agent-max-json-request-characters (* 256 1024)
  "Largest encoded JSON request accepted by the in-process wire adapter.")

(defconst vela-agent-max-json-request-nodes 1024
  "Largest decoded value-node count accepted by the JSON wire adapter.")

(defconst vela-agent-max-json-frame-bytes (* 256 1024)
  "Largest raw newline-delimited JSON frame accepted before decoding.")

(defconst vela-agent-max-json-frames-per-feed 16
  "Largest number of complete JSON frames accepted by one framer feed.")

(defconst vela-agent-max-json-feed-bytes
  (+ (* vela-agent-max-json-frames-per-feed
        (1+ vela-agent-max-json-frame-bytes))
     vela-agent-max-json-frame-bytes)
  "Largest raw chunk accepted by one JSON framer feed.

This admits the maximum number of bounded frames and delimiters followed by one
maximum-size partial frame.")

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
    "project" 'project-current "read-only native project root metadata" "project")
   (vela-agent--emacs-feature
    "diagnostics" 'flymake-diagnostics
    "read-only current-line Flymake diagnostic metadata" "diagnostics")
   (vela-agent--emacs-feature
    "compilation" 'compilation-start
    "read-only current-buffer compilation progress counts" "compilation")
   (vela-agent--emacs-feature
    "magit" 'magit-status "read-only current-buffer Magit mode metadata" "magit")))

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

(defun vela-agent--buffer-identity-state ()
  "Return process-lifetime buffer identity state that survives feature reload."
  (or (get 'vela-agent-mode 'vela-agent--buffer-identity-state)
      (let ((state (cons (make-hash-table :test #'eq :weakness 'key) 0)))
        (put 'vela-agent-mode 'vela-agent--buffer-identity-state state)
        state)))

(defun vela-agent--buffer-identity ()
  "Return an opaque process-local identity for the current live buffer."
  (let* ((state (vela-agent--buffer-identity-state))
         (identities (car state)))
    (or (gethash (current-buffer) identities)
        (let ((identity (format "vela-buffer-%d" (1+ (cdr state)))))
          (setcdr state (1+ (cdr state)))
          (puthash (current-buffer) identity identities)
          identity))))

(defun vela-agent--buffer-context ()
  "Snapshot bounded metadata for the current buffer without moving point."
  `(("name" . ,(vela-agent--bounded-metadata-string (buffer-name)))
    ("file" . ,(if buffer-file-name
                    (vela-agent--bounded-metadata-string buffer-file-name)
                  :null))
    ("identity" . ,(vela-agent--buffer-identity))
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

(defun vela-agent--project-context ()
  "Snapshot the bounded native project root for the current buffer."
  (save-match-data
    (let ((project (project-current nil)))
      (if project
          (let ((root (project-root project)))
            (unless (and (stringp root) (file-name-absolute-p root))
              (signal 'vela-agent-protocol-error
                      '("native project root must be an absolute path")))
            `(("root" . ,(vela-agent--bounded-metadata-string root))))
        :null))))

(defun vela-agent--diagnostic-type-string (type)
  "Return bounded protocol text for Flymake diagnostic TYPE."
  (unless (symbolp type)
    (signal 'vela-agent-protocol-error
            '("Flymake diagnostic type must be a symbol")))
  (let ((name (symbol-name type)))
    (vela-agent--bounded-metadata-string
     (if (string-prefix-p ":" name) (substring name 1) name))))

(defun vela-agent--diagnostic-context-item (diagnostic line-start line-end)
  "Validate and convert DIAGNOSTIC intersecting LINE-START and LINE-END."
  (let ((buffer (flymake-diagnostic-buffer diagnostic))
        (start (flymake-diagnostic-beg diagnostic))
        (end (flymake-diagnostic-end diagnostic))
        (type (flymake-diagnostic-type diagnostic))
        (text (flymake-diagnostic-text diagnostic)))
    (unless (eq buffer (current-buffer))
      (signal 'vela-agent-protocol-error
              '("Flymake diagnostic belongs to another buffer")))
    (dolist (bound (list start end))
      (when (and (markerp bound)
                 (not (eq (marker-buffer bound) (current-buffer))))
        (signal 'vela-agent-protocol-error
                '("Flymake diagnostic marker belongs to another buffer"))))
    (setq start (if (markerp start) (marker-position start) start)
          end (if (markerp end) (marker-position end) end))
    (unless (and (integerp start)
                 (integerp end)
                 (<= (point-min) start)
                 (< start end)
                 (<= end (point-max))
                 (< start line-end)
                 (> end line-start))
      (signal 'vela-agent-protocol-error
              '("Flymake diagnostic has invalid accessible line bounds")))
    `(("start" . ,start)
      ("end" . ,end)
      ("type" . ,(vela-agent--diagnostic-type-string type))
      ("text" . ,(vela-agent--bounded-metadata-string text)))))

(defun vela-agent--diagnostic-item-less-p (left right)
  "Return non-nil when diagnostic item LEFT sorts before RIGHT."
  (let ((left-start (alist-get "start" left nil nil #'string=))
        (right-start (alist-get "start" right nil nil #'string=))
        (left-end (alist-get "end" left nil nil #'string=))
        (right-end (alist-get "end" right nil nil #'string=))
        (left-type (alist-get "type" left nil nil #'string=))
        (right-type (alist-get "type" right nil nil #'string=))
        (left-text (alist-get "text" left nil nil #'string=))
        (right-text (alist-get "text" right nil nil #'string=)))
    (cond
     ((/= left-start right-start) (< left-start right-start))
     ((/= left-end right-end) (< left-end right-end))
     ((not (string= left-type right-type)) (string-lessp left-type right-type))
     (t (string-lessp left-text right-text)))))

(defun vela-agent--diagnostics-context ()
  "Snapshot bounded published Flymake diagnostics for the accessible line."
  (save-match-data
    (save-excursion
      (let* ((line-start (line-beginning-position))
             (line-end (min (point-max) (1+ (line-end-position))))
             (cursor (flymake-diagnostics line-start line-end))
             (count 0)
             ;; Include the JSON array brackets before adding each item and
             ;; the comma that precedes every item after the first.
             (encoded-characters 2)
             items)
        (while (consp cursor)
          (when (>= count vela-agent-max-json-collection-items)
            (signal 'vela-agent-protocol-error
                    '("Flymake diagnostics exceed the collection bound")))
          (let* ((item (vela-agent--diagnostic-context-item
                        (car cursor) line-start line-end))
                 (item-characters
                  (length
                   (vela-agent--json-serialize
                    item 0 (make-hash-table :test #'eq) (vector 0)))))
            (setq encoded-characters
                  (+ encoded-characters item-characters (if (> count 0) 1 0)))
            (when (> encoded-characters
                     vela-agent-max-diagnostics-json-characters)
              (signal 'vela-agent-protocol-error
                      '("Flymake diagnostics exceed the aggregate JSON bound")))
            (push item items))
          (setq cursor (cdr cursor)
                count (1+ count)))
        (unless (null cursor)
          (signal 'vela-agent-protocol-error
                  '("Flymake diagnostics must be a proper list")))
        (vconcat (sort items #'vela-agent--diagnostic-item-less-p))))))

(defun vela-agent--compilation-count (variable)
  "Return bounded current-buffer compilation counter VARIABLE."
  (unless (local-variable-p variable (current-buffer))
    (signal 'vela-agent-protocol-error
            '("native compilation counter must be buffer-local")))
  (let ((value (symbol-value variable)))
    (unless (and (natnump value)
                 (<= value vela-agent-max-compilation-count))
      (signal 'vela-agent-protocol-error
              '("native compilation counter exceeds the response bound")))
    value))

(defun vela-agent--compilation-context ()
  "Snapshot bounded native state for the current compilation buffer."
  (save-match-data
    (if (and (fboundp 'compilation-buffer-p)
             (compilation-buffer-p (current-buffer)))
        (let ((process (get-buffer-process (current-buffer))))
          `(("process_active" . ,(vela-agent--boolean
                                   (and process (process-live-p process))))
            ("errors" . ,(vela-agent--compilation-count
                           'compilation-num-errors-found))
            ("warnings" . ,(vela-agent--compilation-count
                             'compilation-num-warnings-found))
            ("infos" . ,(vela-agent--compilation-count
                          'compilation-num-infos-found))))
      :null)))

(defun vela-agent--magit-context ()
  "Snapshot bounded mode metadata for the current loaded Magit buffer."
  (save-match-data
    (if (and (fboundp 'magit-status)
             (derived-mode-p 'magit-mode))
        (progn
          (unless (symbolp major-mode)
            (signal 'vela-agent-protocol-error
                    '("native Magit major mode must be a symbol")))
          `(("major_mode" . ,(vela-agent--bounded-metadata-string
                              (symbol-name major-mode)))))
      :null)))

(defun vela-agent--record-unique-section (section seen)
  "Record bounded SECTION in SEEN, or reject an existing entry."
  (when (gethash section seen)
    (signal 'vela-agent-protocol-error
            '("context.snapshot sections must be unique")))
  (puthash section t seen))

(defun vela-agent--context-snapshot (request)
  "Return only the explicitly requested context sections from REQUEST."
  (let ((include (alist-get "include" request nil nil #'string=)))
    (unless (vectorp include)
      (signal 'vela-agent-protocol-error
              '("context.snapshot requires an include vector")))
    (when (> (length include) 6)
      (signal 'vela-agent-protocol-error
              '("context.snapshot accepts at most six sections")))
    (let ((sections (append include nil)))
      (let ((seen (make-hash-table :test #'equal)))
        (dolist (section sections)
          (unless (and (stringp section)
                       (<= (length section) vela-agent-max-operation-characters)
                       (member section
                               '("buffer" "org" "project" "diagnostics"
                                 "compilation" "magit")))
            (signal 'vela-agent-protocol-error
                    '("unsupported context section")))
          (vela-agent--record-unique-section section seen)))
      (when (> (buffer-size) vela-agent-max-buffer-characters)
        (signal 'vela-agent-protocol-error
                (list (format "buffer exceeds context snapshot limit: %d characters"
                              vela-agent-max-buffer-characters))))
      (let (result)
        (when (member "buffer" sections)
          (push (cons "buffer" (vela-agent--buffer-context)) result))
        (when (member "org" sections)
          (push (cons "org" (vela-agent--org-context)) result))
        (when (member "project" sections)
          (push (cons "project" (vela-agent--project-context)) result))
        (when (member "diagnostics" sections)
          (push (cons "diagnostics" (vela-agent--diagnostics-context)) result))
        (when (member "compilation" sections)
          (push (cons "compilation" (vela-agent--compilation-context)) result))
        (when (member "magit" sections)
          (push (cons "magit" (vela-agent--magit-context)) result))
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

(defun vela-agent--validate-decoded-json (value depth node-count)
  "Reject ambiguous or deeply nested decoded JSON VALUE.

DEPTH and NODE-COUNT bound recursive validation independently of encoded size."
  (when (> depth vela-agent-max-json-depth)
    (signal 'vela-agent-protocol-error
            '("JSON request exceeds the nesting-depth bound")))
  (aset node-count 0 (1+ (aref node-count 0)))
  (when (> (aref node-count 0) vela-agent-max-json-request-nodes)
    (signal 'vela-agent-protocol-error
            '("JSON request exceeds the value-node bound")))
  (cond
   ((vectorp value)
    (when (> (length value) vela-agent-max-json-collection-items)
      (signal 'vela-agent-protocol-error
              '("JSON request array exceeds the collection bound")))
    (dotimes (index (length value))
      (vela-agent--validate-decoded-json
       (aref value index) (1+ depth) node-count)))
   ((listp value)
    (let ((cursor value)
          (count 0)
          (keys (make-hash-table :test #'equal)))
      (while (consp cursor)
        (when (>= count vela-agent-max-json-collection-items)
          (signal 'vela-agent-protocol-error
                  '("JSON request object exceeds the collection bound")))
        (let ((entry (car cursor)))
          (unless (and (consp entry) (stringp (car entry)))
            (signal 'vela-agent-protocol-error
                    '("decoded JSON request object is malformed")))
          (when (gethash (car entry) keys)
            (signal 'vela-agent-protocol-error
                    '("JSON request object keys must be unique")))
          (puthash (car entry) t keys)
          (vela-agent--validate-decoded-json
           (cdr entry) (1+ depth) node-count))
        (setq cursor (cdr cursor)
              count (1+ count)))
      (unless (null cursor)
        (signal 'vela-agent-protocol-error
                '("decoded JSON request object must be proper")))))
   ((or (eq value :null) (eq value :false) (eq value t)
        (stringp value) (numberp value)) nil)
   (t
    (signal 'vela-agent-protocol-error
            '("decoded request value is not JSON-compatible")))))

(defun vela-agent--decode-json-frame (bytes)
  "Strictly decode one non-empty unibyte JSON frame BYTES as UTF-8."
  (when (and (> (length bytes) 0)
             (= (aref bytes (1- (length bytes))) ?\r))
    (setq bytes (substring bytes 0 -1)))
  (when (= (length bytes) 0)
    (signal 'vela-agent-protocol-error '("JSON frames must not be empty")))
  (condition-case nil
      (let ((decoded (decode-coding-string bytes 'utf-8 t)))
        ;; Emacs can preserve malformed byte sequences as raw characters.
        ;; Reject that eight-bit preservation before checking canonical bytes.
        (dotimes (index (length decoded))
          (let ((character (aref decoded index)))
            (when (or (eq (char-charset character) 'eight-bit)
                      (> character #x10ffff)
                      (<= #xd800 character #xdfff))
              (signal 'vela-agent-protocol-error
                      '("JSON frame is not valid Unicode UTF-8")))))
        ;; Canonical UTF-8 must survive an exact decode/encode round trip.
        (unless (equal (encode-coding-string decoded 'utf-8 t) bytes)
          (signal 'vela-agent-protocol-error
                  '("JSON frame is not canonical UTF-8")))
        decoded)
    (vela-agent-protocol-error
     (signal 'vela-agent-protocol-error '("JSON frame is not canonical UTF-8")))
    (error
     (signal 'vela-agent-protocol-error '("JSON frame is not valid UTF-8")))))

(defun vela-agent-json-frame-encode (payload)
  "Encode one bounded Emacs JSON PAYLOAD as a newline frame.

The returned string contains canonical unibyte UTF-8 followed by one LF.  This
pure helper does not parse JSON, dispatch requests, or own transport state."
  (unless (stringp payload)
    (signal 'vela-agent-protocol-error
            '("JSON frame payload must be a string")))
  (when (> (length payload) vela-agent-max-json-frame-bytes)
    (signal 'vela-agent-protocol-error
            '("JSON frame payload exceeds the character preflight bound")))
  (when (or (string-search "\n" payload) (string-search "\r" payload))
    (signal 'vela-agent-protocol-error
            '("JSON frame payload must not contain delimiters")))
  (dotimes (index (length payload))
    (let ((character (aref payload index)))
      (when (or (eq (char-charset character) 'eight-bit)
                (> character #x10ffff)
                (<= #xd800 character #xdfff))
        (signal 'vela-agent-protocol-error
                '("JSON frame payload is not valid Unicode")))))
  (let ((bytes
         (condition-case nil
             (encode-coding-string payload 'utf-8)
           (error
            (signal 'vela-agent-protocol-error
                    '("JSON frame payload is not valid Unicode"))))))
    (unless
        (equal
         (condition-case nil
             (decode-coding-string bytes 'utf-8 t)
           (error
            (signal 'vela-agent-protocol-error
                    '("JSON frame payload is not valid Unicode"))))
         payload)
      (signal 'vela-agent-protocol-error
              '("JSON frame payload is not canonical Unicode")))
    (when (> (length bytes) vela-agent-max-json-frame-bytes)
      (signal 'vela-agent-protocol-error
              '("encoded JSON frame exceeds the byte bound")))
    (concat bytes (unibyte-string ?\n))))

(defun vela-agent-json-frame-feed (pending chunk)
  "Split bounded raw PENDING and CHUNK bytes into newline JSON frames.

Both arguments must be unibyte strings.  The returned ordered object contains a
`frames' vector of strictly decoded UTF-8 strings and an unibyte `remainder'
for the caller to supply as PENDING on its next feed.  This pure framing helper
does not dispatch requests or own transport state."
  (unless (and (stringp pending) (not (multibyte-string-p pending))
               (stringp chunk) (not (multibyte-string-p chunk)))
    (signal 'vela-agent-protocol-error
            '("JSON framing accepts only unibyte strings")))
  (when (> (length pending) vela-agent-max-json-frame-bytes)
    (signal 'vela-agent-protocol-error
            '("partial JSON frame exceeds the byte bound")))
  (when (> (length chunk) vela-agent-max-json-feed-bytes)
    (signal 'vela-agent-protocol-error
            '("JSON framing chunk exceeds the feed byte bound")))
  (let* ((bytes (concat pending chunk))
         (start 0)
         (count 0)
         frames
         delimiter)
    (while (setq delimiter (string-search "\n" bytes start))
      (when (>= count vela-agent-max-json-frames-per-feed)
        (signal 'vela-agent-protocol-error
                '("JSON framing feed has too many complete frames")))
      (when (> (- delimiter start) vela-agent-max-json-frame-bytes)
        (signal 'vela-agent-protocol-error
                '("complete JSON frame exceeds the byte bound")))
      (push (vela-agent--decode-json-frame (substring bytes start delimiter))
            frames)
      (setq count (1+ count)
            start (1+ delimiter)))
    (let ((remainder (substring bytes start)))
      (when (> (length remainder) vela-agent-max-json-frame-bytes)
        (signal 'vela-agent-protocol-error
                '("partial JSON frame exceeds the byte bound")))
      `(("frames" . ,(vconcat (nreverse frames)))
        ("remainder" . ,remainder)))))

(defun vela-agent-handle-json (encoded-request)
  "Decode, dispatch, and encode one bounded JSON ENCODED-REQUEST.

This is an in-process wire adapter only.  It provides no framing, transport,
queue, asynchronous job, or mutation authority."
  (unless (eq (current-thread) vela-agent--editor-thread)
    (signal 'vela-agent-protocol-error
            '("agent requests must run on the editor owner thread")))
  (unless (and (stringp encoded-request)
               (<= (length encoded-request)
                   vela-agent-max-json-request-characters))
    (signal 'vela-agent-protocol-error
            '("encoded JSON request exceeds the input bound")))
  (let ((request
         (condition-case nil
             (with-temp-buffer
               (insert encoded-request)
               (goto-char (point-min))
               ;; `json-read' preserves duplicate object members but accepts
               ;; some non-standard syntax.  Validate strict JSON first, then
               ;; decode to the ordered alists required by request validation.
               (ignore (json-parse-string encoded-request))
               (let ((json-object-type 'alist)
                     (json-key-type 'string)
                     (json-array-type 'vector)
                     (json-null :null)
                     (json-false :false))
                 (prog1 (json-read)
                   (skip-chars-forward " \t\r\n")
                   (unless (eobp)
                     (error "trailing JSON input")))))
           (error
            (signal 'vela-agent-protocol-error
                    '("encoded JSON request is malformed"))))))
    (unless (listp request)
      (signal 'vela-agent-protocol-error
              '("encoded JSON request must be an object")))
    (vela-agent--validate-decoded-json request 0 (vector 0))
    (vela-agent-encode-response (vela-agent-handle-request request))))

(defun vela-agent-handle-json-feed (pending chunk)
  "Handle complete bounded JSON requests from raw PENDING and CHUNK bytes.

The returned ordered object contains an unibyte `responses' vector and the exact
unibyte `remainder'.  The caller owns the remainder and all transport policy.
Any framing, request, dispatch, or response error rejects the complete feed."
  (unless (eq (current-thread) vela-agent--editor-thread)
    (signal 'vela-agent-protocol-error
            '("framed agent requests must run on the editor owner thread")))
  (let* ((feed (vela-agent-json-frame-feed pending chunk))
         (requests (alist-get "frames" feed nil nil #'string=))
         responses)
    (dotimes (index (length requests))
      (push
       (vela-agent-json-frame-encode
        (vela-agent-handle-json (aref requests index)))
       responses))
    `(("responses" . ,(vconcat (nreverse responses)))
      ("remainder" . ,(alist-get "remainder" feed nil nil #'string=)))))

(defun vela-agent-interface-refresh ()
  "Refresh the interface from `vela-agent-interface-source-buffer'."
  (interactive)
  (unless (buffer-live-p vela-agent-interface-source-buffer)
    (user-error "The Vela agent source buffer is no longer live"))
  (let ((response
         (with-current-buffer vela-agent-interface-source-buffer
           (vela-agent-handle-request
            '(("operation" . "context.snapshot")
              ("include" . ["buffer" "org" "project" "diagnostics"
                            "compilation"]))))))
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
