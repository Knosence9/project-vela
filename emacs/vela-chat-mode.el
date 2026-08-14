;;; vela-chat-mode.el --- Asynchronous Vela gateway chat -*- lexical-binding: t; -*-

;; Copyright (C) 2026 Project Vela contributors
;; SPDX-License-Identifier: MIT
;; Package-Requires: ((emacs "30.1"))
;; Keywords: comm, tools, convenience

;;; Commentary:

;; `vela-chat-mode' is a human-facing transcript and composer for Vela's client
;; gateway contract.  It deliberately remains separate from the read-only agent
;; semantic protocol.  HTTP and SSE retrieval are asynchronous, while bounded
;; parsing and display updates run as short callbacks on Emacs's editor thread.
;; Credentials are obtained on demand from a caller-supplied function and are
;; never persisted in the chat buffer.

;;; Code:

(require 'cl-lib)
(require 'json)
(require 'subr-x)
(require 'url)
(require 'url-http)
(require 'url-parse)

(defvar url-http-response-status)
(defvar url-http-end-of-headers)

(define-error 'vela-chat-error "Vela chat failed")

(defgroup vela-chat nil
  "Human-facing chat with the Vela client gateway."
  :group 'applications)

(defcustom vela-chat-base-url "http://127.0.0.1:3847"
  "Origin of the Vela client gateway.

Only an HTTP or HTTPS origin without credentials, query, fragment, or a
non-root path is accepted."
  :type 'string
  :group 'vela-chat)

(defcustom vela-chat-session-mode "canonical"
  "Gateway session mode requested by a new chat buffer."
  :type '(choice (const "canonical") (const "isolated"))
  :group 'vela-chat)

(defcustom vela-chat-auth-source-host "vela-gateway"
  "Auth-source host used by `vela-chat-auth-source-token'."
  :type 'string
  :group 'vela-chat)

(defcustom vela-chat-auth-token-function nil
  "Function returning a runtime bearer token, or nil.

The default performs no credential lookup.  Configure this explicitly, for
example as `vela-chat-auth-source-token', to opt in to bearer authentication.
The token is requested separately for each HTTP operation.  Vela chat never
persists or displays the returned value."
  :type '(choice (const :tag "No bearer token" nil) function)
  :group 'vela-chat)

(defcustom vela-chat-operation-timeout-seconds 30
  "Maximum seconds allowed for one resolve or turn HTTP operation."
  :type 'number
  :group 'vela-chat)

(defcustom vela-chat-sse-idle-timeout-seconds 60
  "Maximum seconds an open gateway stream may receive no transport bytes."
  :type 'number
  :group 'vela-chat)

(defconst vela-chat-max-input-characters 32768
  "Largest user message accepted by `vela-chat-send'.")

(defconst vela-chat-max-timeout-seconds 86400
  "Largest configurable chat operation or idle timeout.")

(defconst vela-chat-max-http-response-bytes (* 256 1024)
  "Largest HTTP JSON response body accepted by Vela chat.")

(defconst vela-chat-max-http-header-bytes (* 64 1024)
  "Largest HTTP header allowance before a bounded JSON response.")

(defconst vela-chat-max-http-request-bytes (* 64 1024)
  "Largest encoded HTTP JSON request body emitted by Vela chat.")

(defconst vela-chat-max-sse-event-bytes (* 256 1024)
  "Largest assembled SSE data field accepted by Vela chat.")

(defconst vela-chat-max-sse-response-bytes (* 8 1024 1024)
  "Largest aggregate SSE response accepted for one transport.")

(defconst vela-chat-max-sse-pending-bytes (* 64 1024)
  "Largest unterminated SSE line retained between chunks.")

(defconst vela-chat-max-events-per-turn 1024
  "Largest number of SSE events accepted for one chat turn.")

(defconst vela-chat-max-transcript-characters (* 1024 1024)
  "Largest live Vela chat transcript buffer.")

(defconst vela-chat-max-token-characters 8192
  "Largest runtime bearer token accepted by Vela chat.")

(defconst vela-chat-max-label-characters 256
  "Largest gateway-provided transcript label component.")

(defconst vela-chat-max-json-depth 16
  "Largest decoded gateway JSON nesting depth.")

(defconst vela-chat-max-json-nodes 1024
  "Largest decoded gateway JSON value-node count.")

(defconst vela-chat-sse-poll-seconds 0.05
  "Interval used to observe asynchronous URL retrieval buffers.")

(defface vela-chat-title-face
  '((t :inherit font-lock-keyword-face :weight bold :height 1.15))
  "Face for the Vela chat title."
  :group 'vela-chat)

(defface vela-chat-human-face
  '((t :inherit font-lock-function-name-face :weight bold))
  "Face for human transcript labels."
  :group 'vela-chat)

(defface vela-chat-assistant-face
  '((t :inherit font-lock-keyword-face :weight bold))
  "Face for assistant transcript labels."
  :group 'vela-chat)

(defface vela-chat-status-face
  '((t :inherit shadow :slant italic))
  "Face for thinking, tool, and status transcript labels."
  :group 'vela-chat)

(defface vela-chat-error-face
  '((t :inherit error :weight bold))
  "Face for failed or degraded transcript labels."
  :group 'vela-chat)

(defvar-keymap vela-chat-mode-map
  :doc "Keymap for `vela-chat-mode'."
  "RET" #'vela-chat-send
  "C-c C-c" #'vela-chat-send
  "C-c C-k" #'vela-chat-cancel
  "C-c C-n" #'vela-chat-new-session
  "C-c C-q" #'quit-window)

(defvar-local vela-chat-post-json-function #'vela-chat--url-post-json
  "Asynchronous JSON POST adapter used by the current chat buffer.")

(defvar-local vela-chat-stream-function #'vela-chat--url-stream
  "Asynchronous SSE adapter used by the current chat buffer.")

(defvar-local vela-chat--session-id nil)
(defvar-local vela-chat--turn-id nil)
(defvar-local vela-chat--busy nil)
(defvar-local vela-chat--terminal nil)
(defvar-local vela-chat--generation 0)
(defvar-local vela-chat--event-count 0)
(defvar-local vela-chat--active-handle nil)
(defvar-local vela-chat--transport-stage nil)
(defvar-local vela-chat--timeout-timer nil)
(defvar-local vela-chat--timeout-serial 0)
(defvar-local vela-chat--input-start nil)
(defvar-local vela-chat--assistant-message-id nil)
(defvar-local vela-chat--assistant-start nil)
(defvar-local vela-chat--assistant-end nil)

(defvar vela-chat--runtime-token-override nil
  "Dynamically bound prevalidated token for one synchronous request setup.")

(cl-defstruct
    (vela-chat--sse-parser (:constructor vela-chat--sse-parser-create))
  (pending "")
  (at-stream-start t)
  (skip-leading-lf nil)
  event
  (data-lines nil)
  (data-characters 0)
  (event-count 0)
  (response-bytes 0))

(defun vela-chat--ensure-owner-thread ()
  "Reject entry outside Emacs's main editor thread."
  (unless (eq (current-thread) main-thread)
    (signal 'vela-chat-error '("chat callbacks require the editor thread"))))

(defun vela-chat--field (name object)
  "Return string-keyed NAME from decoded alist OBJECT."
  (and (listp object) (alist-get name object nil nil #'string=)))

(defun vela-chat--required-object (name object)
  "Return required alist field NAME from OBJECT."
  (let ((entry (and (listp object) (assoc-string name object))))
    (unless (and entry (listp (cdr entry)))
      (signal 'vela-chat-error (list (format "missing gateway object: %s" name))))
    (cdr entry)))

(defun vela-chat--required-string (name object)
  "Return required non-empty string field NAME from OBJECT."
  (let ((value (vela-chat--field name object)))
    (unless (and (stringp value) (not (string-empty-p value)))
      (signal 'vela-chat-error (list (format "missing gateway string: %s" name))))
    value))

(defun vela-chat--required-label (name object)
  "Return required bounded single-line label field NAME from OBJECT."
  (let ((value (vela-chat--required-string name object)))
    (when (or (> (length value) vela-chat-max-label-characters)
              (cl-some (lambda (character)
                         (memq (get-char-code-property
                                character 'general-category)
                               '(Cc Cf Zl Zp)))
                       (string-to-list value)))
      (signal 'vela-chat-error
              (list (format "gateway label is invalid: %s" name))))
    value))

(defun vela-chat--effective-port (parsed)
  "Return the effective port for PARSED URL."
  (or (url-port parsed)
      (if (string= (url-type parsed) "https") 443 80)))

(defun vela-chat--unsafe-url-character-p (character)
  "Return non-nil when CHARACTER is forbidden in a gateway URL."
  (or (= character ?\\)
      (memq (get-char-code-property character 'general-category)
            '(Cc Cf Zs Zl Zp))))

(defun vela-chat--unsafe-url-p (url)
  "Return non-nil when URL contains unsafe characters or undecoded non-ASCII bytes."
  (or (and (not (multibyte-string-p url))
           (cl-some (lambda (byte) (> byte 127)) url))
      (cl-some #'vela-chat--unsafe-url-character-p url)))

(defun vela-chat--parse-origin (raw)
  "Parse and validate HTTP(S) origin RAW."
  (unless (and (stringp raw) (not (string-empty-p raw)))
    (signal 'vela-chat-error '("gateway base URL must be a non-empty string")))
  (when (vela-chat--unsafe-url-p raw)
    (signal 'vela-chat-error '("gateway base URL contains unsafe characters")))
  (let* ((case-fold-search t)
         (parsed (url-generic-parse-url raw))
         (scheme (url-type parsed))
         (host (url-host parsed))
         (authority (and (string-match "\\`https?://\\([^/?#]*\\)" raw)
                         (match-string 1 raw)))
         (path (url-filename parsed)))
    (unless (and (member scheme '("http" "https"))
                 (stringp host)
                 (not (string-empty-p host)))
      (signal 'vela-chat-error '("gateway base URL must be an HTTP(S) origin")))
    (when (or (url-user parsed) (url-password parsed))
      (signal 'vela-chat-error '("gateway URLs must not contain credentials")))
    (let ((port-text
           (cond
            ((and authority
                  (string-match "\\`\\[[^][]+\\]\\(?::\\([0-9]*\\)\\)?\\'" authority))
             (match-string 1 authority))
            ((and authority
                  (string-match "\\`[^:@]+\\(?::\\([0-9]*\\)\\)?\\'" authority))
             (match-string 1 authority))
            (t
             (signal 'vela-chat-error '("gateway base URL has a malformed authority"))))))
      (when (and port-text
                 (or (string-empty-p port-text)
                     (not (<= 1 (string-to-number port-text) 65535))))
        (signal 'vela-chat-error '("gateway base URL port must be a decimal integer between 1 and 65535"))))
    (when (or (url-target parsed)
              (and path (not (member path '("" "/")))))
      (signal 'vela-chat-error '("gateway base URL must not contain query, fragment, or path")))
    parsed))

(defun vela-chat--origin-string ()
  "Return normalized configured gateway origin."
  (let ((parsed (vela-chat--parse-origin vela-chat-base-url)))
    (format "%s://%s%s"
            (url-type parsed)
            (url-host parsed)
            (let ((port (url-portspec parsed)))
              (if port (format ":%d" port) "")))))

(defun vela-chat--same-origin-p (left right)
  "Return non-nil when parsed URLs LEFT and RIGHT share an origin."
  (and (string= (url-type left) (url-type right))
       (string= (downcase (url-host left)) (downcase (url-host right)))
       (= (vela-chat--effective-port left) (vela-chat--effective-port right))))

(defun vela-chat--resolve-stream-url (stream-url)
  "Resolve and validate gateway STREAM-URL against the configured origin."
  (unless (and (stringp stream-url) (not (string-empty-p stream-url)))
    (signal 'vela-chat-error '("gateway stream URL must be a non-empty string")))
  (when (vela-chat--unsafe-url-p stream-url)
    (signal 'vela-chat-error '("gateway stream URL contains unsafe characters")))
  (let* ((base (vela-chat--parse-origin vela-chat-base-url))
         (origin (vela-chat--origin-string))
         (absolute
          (cond
           ((string-match-p "\\`https?://" stream-url) stream-url)
           ((and (string-prefix-p "/" stream-url)
                 (not (string-prefix-p "//" stream-url)))
            (concat origin stream-url))
           (t (signal 'vela-chat-error '("gateway stream URL must be absolute-path or HTTP(S)")))))
         (parsed (url-generic-parse-url absolute)))
    (when (or (url-user parsed) (url-password parsed))
      (signal 'vela-chat-error '("gateway stream URL must not contain credentials")))
    (when (url-target parsed)
      (signal 'vela-chat-error '("gateway stream URL must not contain a fragment")))
    (unless (vela-chat--same-origin-p base parsed)
      (signal 'vela-chat-error '("gateway stream URL must remain same-origin")))
    absolute))

(defun vela-chat--runtime-token ()
  "Return a validated runtime token, or nil."
  (let ((token (if (and vela-chat--runtime-token-override
                        (car vela-chat--runtime-token-override))
                   (prog1 (cdr vela-chat--runtime-token-override)
                     (setcar vela-chat--runtime-token-override nil))
                 (and vela-chat-auth-token-function
                      (funcall vela-chat-auth-token-function)))))
    (when token
      (unless (and (stringp token)
                   (not (string-empty-p token))
                   (<= (length token) vela-chat-max-token-characters)
                   (not (cl-some (lambda (character)
                                   (or (< character 32) (= character 127)))
                                 (string-to-list token))))
        (signal 'vela-chat-error '("runtime bearer token is invalid"))))
    token))

(defun vela-chat-auth-source-token ()
  "Return the first auth-source secret for `vela-chat-auth-source-host'."
  (require 'auth-source)
  (let* ((entry (car (auth-source-search
                      :host vela-chat-auth-source-host
                      :max 1
                      :require '(:secret))))
         (secret (and entry (plist-get entry :secret))))
    (cond
     ((functionp secret) (funcall secret))
     ((stringp secret) secret)
     (t nil))))

(defun vela-chat--request-headers (&optional event-stream)
  "Return bounded gateway headers, accepting EVENT-STREAM when non-nil."
  (let ((headers `(("Accept" . ,(if event-stream
                                    "text/event-stream"
                                  "application/json"))))
        (token (vela-chat--runtime-token)))
    (when token
      (push (cons "Authorization" (concat "Bearer " token)) headers))
    headers))

(defun vela-chat--canonical-utf8 (text)
  "Return gateway TEXT as canonical decoded UTF-8.

Accept already decoded multibyte text, but reject Emacs-preserved raw bytes,
surrogates, out-of-range characters, and unibyte input that does not survive an
exact UTF-8 decode/encode round trip."
  (unless (stringp text)
    (signal 'vela-chat-error '("gateway input is not a string")))
  (condition-case nil
      (let ((decoded (if (multibyte-string-p text)
                         text
                       (decode-coding-string text 'utf-8-unix t))))
        (dotimes (index (length decoded))
          (let ((character (aref decoded index)))
            (when (or (eq (char-charset character) 'eight-bit)
                      (> character #x10ffff)
                      (<= #xd800 character #xdfff))
              (signal 'vela-chat-error
                      '("gateway input is not valid Unicode UTF-8")))))
        (when (and (not (multibyte-string-p text))
                   (not (equal (encode-coding-string decoded 'utf-8-unix t)
                               text)))
          (signal 'vela-chat-error
                  '("gateway input is not canonical UTF-8")))
        decoded)
    (vela-chat-error
     (signal 'vela-chat-error '("gateway input is not canonical UTF-8")))
    (error
     (signal 'vela-chat-error '("gateway input is not valid UTF-8")))))

(defun vela-chat--validate-json (value depth nodes)
  "Validate decoded JSON VALUE within DEPTH and mutable NODES bounds."
  (when (> depth vela-chat-max-json-depth)
    (signal 'vela-chat-error '("gateway JSON exceeds nesting bound")))
  (aset nodes 0 (1+ (aref nodes 0)))
  (when (> (aref nodes 0) vela-chat-max-json-nodes)
    (signal 'vela-chat-error '("gateway JSON exceeds node bound")))
  (cond
   ((vectorp value)
    (dotimes (index (length value))
      (vela-chat--validate-json (aref value index) (1+ depth) nodes)))
   ((consp value)
    (let ((seen (make-hash-table :test #'equal)))
      (dolist (entry value)
        (unless (and (consp entry) (stringp (car entry)))
          (signal 'vela-chat-error '("gateway JSON object is malformed")))
        (when (gethash (car entry) seen)
          (signal 'vela-chat-error '("gateway JSON contains duplicate keys")))
        (puthash (car entry) t seen)
        (vela-chat--validate-json (cdr entry) (1+ depth) nodes))))
   ((or (stringp value) (numberp value) (memq value '(t :false :null nil))))
   (t (signal 'vela-chat-error '("gateway JSON contains unsupported values"))))
  value)

(defun vela-chat--parse-json (text)
  "Decode and validate bounded gateway JSON TEXT."
  (condition-case err
      (let ((value (json-parse-string (vela-chat--canonical-utf8 text)
                                      :object-type 'alist
                                      :array-type 'array
                                      :null-object :null
                                      :false-object :false)))
        (vela-chat--validate-json
         (vela-chat--stringify-json-keys value) 0 (vector 0)))
    (vela-chat-error (signal (car err) (cdr err)))
    (error (signal 'vela-chat-error '("gateway returned malformed JSON")))))

(defun vela-chat--stringify-json-keys (value)
  "Return decoded JSON VALUE with every object key represented as a string."
  (cond
   ((vectorp value)
    (vconcat (mapcar #'vela-chat--stringify-json-keys (append value nil))))
   ((consp value)
    (mapcar
     (lambda (entry)
       (unless (and (consp entry) (symbolp (car entry)))
         (signal 'vela-chat-error '("gateway JSON object is malformed")))
       (cons (symbol-name (car entry))
             (vela-chat--stringify-json-keys (cdr entry))))
     value))
   (t value)))

(defun vela-chat--encode-json-request (payload)
  "Encode string-keyed gateway request PAYLOAD as bounded UTF-8 bytes."
  (condition-case err
      (let* ((json-encoding-pretty-print nil)
             (bytes (encode-coding-string (json-encode payload) 'utf-8)))
        (when (> (string-bytes bytes) vela-chat-max-http-request-bytes)
          (signal 'vela-chat-error '("gateway request exceeds byte bound")))
        bytes)
    (vela-chat-error (signal (car err) (cdr err)))
    (error (signal 'vela-chat-error '("gateway request is not JSON encodable")))))

(defun vela-chat--http-body-start ()
  "Return the first body position in the current URL retrieval buffer."
  (unless (and (boundp 'url-http-end-of-headers) url-http-end-of-headers)
    (signal 'vela-chat-error '("gateway response omitted HTTP headers")))
  (let ((position (if (markerp url-http-end-of-headers)
                      (marker-position url-http-end-of-headers)
                    url-http-end-of-headers)))
    (unless (and (integerp position)
                 (<= (point-min) position)
                 (<= position (point-max)))
      (signal 'vela-chat-error '("gateway HTTP header boundary is invalid")))
    (min (point-max) (1+ position))))

(defun vela-chat--http-body-byte-length ()
  "Return the decoded HTTP body byte length in the current retrieval buffer."
  (let ((start (vela-chat--http-body-start)))
    (- (or (position-bytes (point-max)) (1+ (buffer-size)))
       (or (position-bytes start) start))))

(defun vela-chat--content-encoding-present-p (headers)
  "Return non-nil when raw HTTP HEADERS declare content encoding."
  (let ((case-fold-search t))
    (string-match-p
     "\\(?:\\`\\|[\r\n]\\)content-encoding[ \t]*:"
     headers)))

(defun vela-chat--http-token-character-p (character)
  "Return non-nil when CHARACTER is valid in an HTTP token."
  (or (and (<= ?0 character) (<= character ?9))
      (and (<= ?A character) (<= character ?Z))
      (and (<= ?a character) (<= character ?z))
      (memq character '(?! ?# ?$ ?% ?& ?' ?* ?+ ?- ?. ?^ ?_ ?` ?| ?~))))

(defun vela-chat--valid-media-type-p (value expected)
  "Return non-nil when VALUE has media-type essence EXPECTED."
  (let ((position 0)
        (size (length value))
        valid)
    (cl-labels
        ((skip-ows ()
           (while (and (< position size)
                       (memq (aref value position) '(?\s ?\t)))
             (setq position (1+ position))))
         (consume-token ()
           (let ((start position))
             (while (and (< position size)
                         (vela-chat--http-token-character-p
                          (aref value position)))
               (setq position (1+ position)))
             (> position start)))
         (consume-quoted-string ()
           (when (and (< position size) (= (aref value position) ?\"))
             (setq position (1+ position))
             (let (closed invalid)
               (while (and (< position size) (not closed) (not invalid))
                 (let ((character (aref value position)))
                   (cond
                    ((= character ?\")
                     (setq closed t
                           position (1+ position)))
                    ((= character ?\\)
                     (setq position (1+ position))
                     (if (>= position size)
                         (setq invalid t)
                       (let ((quoted (aref value position)))
                         (unless (or (= quoted ?\t)
                                     (<= 32 quoted 126)
                                     (<= 128 quoted 255))
                           (setq invalid t))
                         (setq position (1+ position)))))
                    ((or (= character ?\t)
                         (= character 32)
                         (= character 33)
                         (<= 35 character 91)
                         (<= 93 character 126)
                         (<= 128 character 255))
                     (setq position (1+ position)))
                    (t (setq invalid t)))))
               (and closed (not invalid))))))
      (skip-ows)
      (when (and (<= (+ position (length expected)) size)
                 (string=
                  (downcase
                   (substring value position
                              (+ position (length expected))))
                  expected))
        (setq position (+ position (length expected)))
        (skip-ows)
        (setq valid t)
        (while (and valid (< position size))
          (if (/= (aref value position) ?\;)
              (setq valid nil)
            (setq position (1+ position))
            (skip-ows)
            (unless (consume-token) (setq valid nil))
            (when valid
              (if (or (>= position size) (/= (aref value position) ?=))
                  (setq valid nil)
                (setq position (1+ position))
                (unless (or (consume-token) (consume-quoted-string))
                  (setq valid nil))
                (skip-ows)))))
        (and valid (= position size))))))

(defun vela-chat--valid-content-type-p (headers expected)
  "Return non-nil for one EXPECTED Content-Type in raw HTTP HEADERS."
  (let ((case-fold-search t)
        content-types
        folded
        malformed-content-type)
    (dolist (line (cdr (split-string headers "\r?\n")))
      (when (and (not (string-empty-p line))
                 (memq (aref line 0) '(?\s ?\t)))
        (setq folded t))
      (when (string-match-p "\\`content-type[ \t]+:" line)
        (setq malformed-content-type t))
      (when (string-match "\\`content-type:[ \t]*\\(.*\\)\\'" line)
        (push (match-string 1 line) content-types)))
    (and (not folded)
         (not malformed-content-type)
         (= (length content-types) 1)
         (vela-chat--valid-media-type-p (car content-types) expected))))

(defun vela-chat--valid-sse-content-type-p (headers)
  "Return non-nil for one event-stream Content-Type in raw HTTP HEADERS."
  (vela-chat--valid-content-type-p headers "text/event-stream"))

(defun vela-chat--url-post-json (url payload on-success on-error)
  "POST PAYLOAD to URL and call ON-SUCCESS or ON-ERROR asynchronously."
  (let ((url-request-method "POST")
        (url-request-data (vela-chat--encode-json-request payload))
        (url-request-extra-headers
         (cons '("Content-Type" . "application/json")
               (vela-chat--request-headers)))
        (url-max-redirections 0)
        (header-probe "")
        (received-bytes 0)
        headers-complete buffer done)
    (cl-labels
        ((stop ()
           (when (buffer-live-p buffer)
             (let ((process (get-buffer-process buffer)))
               (when (process-live-p process)
                 (set-process-filter process #'ignore)
                 (set-process-sentinel process #'ignore)
                 (delete-process process)))
             (kill-buffer buffer)))
         (fail (message)
           (unless done
             (setq done t)
             (stop)
             (funcall on-error message))))
      (condition-case err
          (progn
            (setq
             buffer
             (url-retrieve
              url
              (lambda (status)
                (unless done
                  (setq done t)
                  (let ((retrieval (current-buffer)))
                    (unwind-protect
                        (condition-case callback-error
                            (if (plist-get status :error)
                                (funcall on-error "gateway request transport failed")
                              (unless (and (boundp 'url-http-response-status)
                                           (integerp url-http-response-status)
                                           (<= 200 url-http-response-status 299))
                                (signal 'vela-chat-error
                                        (list (format "gateway request returned HTTP %s"
                                                      (if (boundp 'url-http-response-status)
                                                          url-http-response-status
                                                        "unknown")))))
                              (when (> (vela-chat--http-body-byte-length)
                                       vela-chat-max-http-response-bytes)
                                (signal 'vela-chat-error
                                        '("gateway response exceeds byte bound")))
                              (let* ((start (vela-chat--http-body-start))
                                     (body (buffer-substring-no-properties
                                            start (point-max))))
                                (funcall on-success (vela-chat--parse-json body))))
                          (error
                           (funcall on-error (error-message-string callback-error))))
                      (when (buffer-live-p retrieval)
                        (kill-buffer retrieval))))))
              nil t t))
            (when (buffer-live-p buffer)
              (let ((process (get-buffer-process buffer)))
                (when (process-live-p process)
                  (let ((filter (process-filter process)))
                    (set-process-filter
                     process
                     (lambda (source chunk)
                       (setq received-bytes
                             (+ received-bytes (string-bytes chunk)))
                       (let (rejection)
                         (when (> received-bytes
                                  (+ vela-chat-max-http-header-bytes
                                     vela-chat-max-http-response-bytes))
                           (setq rejection
                                 "gateway response exceeds transport byte bound"))
                         (unless (or rejection headers-complete)
                           (setq header-probe (concat header-probe chunk))
                           (let ((header-end
                                  (or (string-match "\r\n\r\n" header-probe)
                                      (string-match "\n\n" header-probe))))
                             (cond
                              (header-end
                               (let ((headers
                                      (substring header-probe 0 (match-end 0))))
                                 (cond
                                  ((> (string-bytes headers)
                                      vela-chat-max-http-header-bytes)
                                   (setq rejection
                                         "gateway response exceeds header byte bound"))
                                  ((vela-chat--content-encoding-present-p headers)
                                   (setq rejection
                                         "gateway content encoding is unsupported"))
                                  ((not (vela-chat--valid-content-type-p
                                         headers "application/json"))
                                   (setq rejection
                                         "gateway JSON content type is unsupported"))
                                  (t
                                   (setq headers-complete t
                                         header-probe "")))))
                              ((> (string-bytes header-probe)
                                  vela-chat-max-http-header-bytes)
                               (setq rejection
                                     "gateway response exceeds header byte bound")))))
                         (if rejection
                             (fail rejection)
                           (funcall filter source chunk)
                           (when (and (not done) (buffer-live-p buffer))
                             (with-current-buffer buffer
                               (when (and (boundp 'url-http-end-of-headers)
                                          url-http-end-of-headers
                                          (> (vela-chat--http-body-byte-length)
                                             vela-chat-max-http-response-bytes))
                                 (fail "gateway response exceeds byte bound"))))))))))))
            (when buffer
              (list :buffer buffer
                    :cancel (lambda ()
                              (unless done
                                (setq done t)
                                (stop))))))
        (error
         (fail (error-message-string err))
         nil)))))

(defun vela-chat--sse-flush (parser events)
  "Flush PARSER into EVENTS and return the resulting list."
  (let ((lines (nreverse (vela-chat--sse-parser-data-lines parser)))
        (event (vela-chat--sse-parser-event parser)))
    (when lines
      (let ((next (1+ (vela-chat--sse-parser-event-count parser))))
        (when (> next vela-chat-max-events-per-turn)
          (signal 'vela-chat-error '("gateway stream exceeds event-count bound")))
        (setf (vela-chat--sse-parser-event-count parser) next))
      (push `(("event" . ,(if (and event (not (string-empty-p event)))
                               event
                             "message"))
              ("data" . ,(string-join lines "\n")))
            events)))
  (setf (vela-chat--sse-parser-event parser) nil
        (vela-chat--sse-parser-data-lines parser) nil
        (vela-chat--sse-parser-data-characters parser) 0)
  events)

(defun vela-chat--sse-line (parser line events)
  "Consume one SSE LINE into PARSER and EVENTS."
  (setq line (vela-chat--canonical-utf8 line))
  (cond
   ((string-empty-p line)
    (vela-chat--sse-flush parser events))
   ((string-prefix-p ":" line) events)
   ((or (string= line "event") (string-prefix-p "event:" line))
    (let ((event (if (string= line "event")
                     ""
                   (string-remove-prefix " " (substring line 6)))))
      (unless (string-match-p "\0" event)
        (setf (vela-chat--sse-parser-event parser) event)))
    events)
   ((or (string= line "data") (string-prefix-p "data:" line))
    (let* ((data (if (string= line "data")
                     ""
                   (string-remove-prefix " " (substring line 5))))
           (next (+ (vela-chat--sse-parser-data-characters parser)
                    (string-bytes data)
                    (if (vela-chat--sse-parser-data-lines parser) 1 0))))
      (when (> next vela-chat-max-sse-event-bytes)
        (signal 'vela-chat-error '("gateway SSE event exceeds byte bound")))
      (push data (vela-chat--sse-parser-data-lines parser))
      (setf (vela-chat--sse-parser-data-characters parser) next)
      events))
   (t events)))

(defun vela-chat--sse-feed (parser chunk final)
  "Feed string CHUNK to PARSER and return complete events as a vector.

When FINAL is non-nil, discard any event not terminated by a blank line."
  (unless (and (vela-chat--sse-parser-p parser) (stringp chunk))
    (signal 'vela-chat-error '("invalid SSE parser input")))
  (setf (vela-chat--sse-parser-response-bytes parser)
        (+ (vela-chat--sse-parser-response-bytes parser)
           (string-bytes chunk)))
  (when (> (vela-chat--sse-parser-response-bytes parser)
           vela-chat-max-sse-response-bytes)
    (signal 'vela-chat-error '("gateway SSE response exceeds byte bound")))
  (when (and (vela-chat--sse-parser-skip-leading-lf parser)
             (not (string-empty-p chunk)))
    (when (= (aref chunk 0) ?\n)
      (setq chunk (substring chunk 1)))
    (setf (vela-chat--sse-parser-skip-leading-lf parser) nil))
  (let* ((text (concat (vela-chat--sse-parser-pending parser) chunk))
         (start 0)
         events)
    (when (and (vela-chat--sse-parser-at-stream-start parser)
               (not (string-empty-p text)))
      (cond
       ((multibyte-string-p text)
        (setf (vela-chat--sse-parser-at-stream-start parser) nil)
        (when (= (aref text 0) #xfeff)
          (setq text (substring text 1))))
       ((and (< (length text) 3)
             (= (aref text 0) #xef)
             (or (= (length text) 1)
                 (= (aref text 1) #xbb))))
       (t
        (setf (vela-chat--sse-parser-at-stream-start parser) nil)
        (when (and (>= (length text) 3)
                   (= (aref text 0) #xef)
                   (= (aref text 1) #xbb)
                   (= (aref text 2) #xbf))
          (setq text (substring text 3))))))
    (while (string-match "[\r\n]" text start)
      (let* ((terminator (match-beginning 0))
             (line (substring text start terminator))
             (cr (= (aref text terminator) ?\r))
             (next-start
              (if (and cr
                       (< (1+ terminator) (length text))
                       (= (aref text (1+ terminator)) ?\n))
                  (+ terminator 2)
                (1+ terminator))))
        (when (and cr
                   (= terminator (1- (length text)))
                   (not final))
          (setf (vela-chat--sse-parser-skip-leading-lf parser) t))
        (setq events (vela-chat--sse-line parser line events)
              start next-start)))
    (let ((pending (substring text start)))
      (when (> (string-bytes pending) vela-chat-max-sse-pending-bytes)
        (signal 'vela-chat-error '("gateway SSE line exceeds pending byte bound")))
      (setf (vela-chat--sse-parser-pending parser) pending))
    (when final
      (setf (vela-chat--sse-parser-pending parser) ""
            (vela-chat--sse-parser-skip-leading-lf parser) nil
            (vela-chat--sse-parser-event parser) nil
            (vela-chat--sse-parser-data-lines parser) nil
            (vela-chat--sse-parser-data-characters parser) 0))
    (vconcat (nreverse events))))

(defun vela-chat--url-stream (url on-event on-complete on-error &optional on-activity)
  "Retrieve SSE URL and invoke ON-EVENT, ON-COMPLETE, or ON-ERROR.

The URL package owns HTTP decoding.  A bounded wrapper rejects oversized raw
transport input before delegating to URL's filter, while a timer consumes the
decoded body incrementally.  Invoke ON-ACTIVITY for every received transport
chunk when that optional callback is non-nil."
  (let ((parser (vela-chat--sse-parser-create))
        (header-probe "")
        (received-bytes 0)
        headers-complete buffer timer cursor done)
    (cl-labels
        ((stop ()
           (unless done
             (setq done t)
             (when (timerp timer) (cancel-timer timer))
             (when (markerp cursor) (set-marker cursor nil))
             (when (buffer-live-p buffer)
               (let ((process (get-buffer-process buffer)))
                 (when (process-live-p process)
                   (set-process-filter process #'ignore)
                   (set-process-sentinel process #'ignore)
                   (delete-process process)))
               (kill-buffer buffer))))
         (dispatch (events)
           (let ((index 0))
             (while (and (not done) (< index (length events)))
               (funcall on-event (aref events index))
               (setq index (1+ index)))))
         (poll (&optional final)
           (unless done
             (condition-case err
                 (when (buffer-live-p buffer)
                   (with-current-buffer buffer
                     (let ((header-end (and (boundp 'url-http-end-of-headers)
                                            url-http-end-of-headers)))
                       (when header-end
                         (unless (and (boundp 'url-http-response-status)
                                      (integerp url-http-response-status))
                           (signal 'vela-chat-error
                                   '("gateway stream omitted HTTP status")))
                         (unless (<= 200 url-http-response-status 299)
                           (signal 'vela-chat-error
                                   (list (format "gateway stream returned HTTP %s"
                                                 url-http-response-status))))
                         (unless cursor
                           (setq cursor
                                 (copy-marker (vela-chat--http-body-start))))
                         (when (< cursor (point-max))
                           (let* ((end (point-max))
                                  (events
                                   (vela-chat--sse-feed
                                    parser
                                    (buffer-substring-no-properties cursor end)
                                    final)))
                             (set-marker cursor end)
                             (dispatch events)))
                         (when (and (not done)
                                    final
                                    (marker-buffer cursor)
                                    (= cursor (point-max)))
                           (let ((events (vela-chat--sse-feed parser "" t)))
                             (dispatch events)))))))
               (error
                (unless done
                  (stop)
                  (funcall on-error (error-message-string err))))))))
      (let ((url-request-method "GET")
            (url-request-extra-headers (vela-chat--request-headers t))
            (url-max-redirections 0))
        (condition-case err
            (setq buffer
                  (url-retrieve
                   url
                   (lambda (status)
                     (unless done
                       (if (plist-get status :error)
                           (progn
                             (stop)
                             (funcall on-error "gateway stream transport failed"))
                         (condition-case callback-error
                             (progn
                               (unless (and (boundp 'url-http-response-status)
                                            (integerp url-http-response-status)
                                            (<= 200 url-http-response-status 299))
                                 (signal 'vela-chat-error
                                         (list (format "gateway stream returned HTTP %s"
                                                       (if (boundp 'url-http-response-status)
                                                           url-http-response-status
                                                         "unknown")))))
                               (poll t)
                               (unless done
                                 (stop)
                                 (funcall on-complete)))
                           (error
                            (stop)
                            (funcall on-error
                                     (error-message-string callback-error)))))))
                   nil t t))
          (error
           (funcall on-error (error-message-string err))
           (setq done t)))
        (when (and (buffer-live-p buffer) (not done))
          (let ((process (get-buffer-process buffer)))
            (when (process-live-p process)
              (let ((filter (process-filter process)))
                (set-process-filter
                 process
                 (lambda (source chunk)
                   (setq received-bytes
                         (+ received-bytes (string-bytes chunk)))
                   (when on-activity (funcall on-activity))
                   (cond
                    ((> received-bytes
                        (+ vela-chat-max-http-header-bytes
                           vela-chat-max-sse-response-bytes))
                     (stop)
                     (funcall on-error
                              "gateway stream exceeds transport byte bound"))
                    ((not headers-complete)
                     (setq header-probe (concat header-probe chunk))
                     (let ((header-end
                            (or (string-match "\r\n\r\n" header-probe)
                                (string-match "\n\n" header-probe))))
                       (cond
                        (header-end
                         (let ((headers
                                (substring header-probe 0 (match-end 0))))
                           (cond
                            ((> (string-bytes headers)
                                vela-chat-max-http-header-bytes)
                             (stop)
                             (funcall on-error
                                      "gateway stream exceeds header byte bound"))
                            ((vela-chat--content-encoding-present-p headers)
                             (stop)
                             (funcall on-error
                                      "gateway content encoding is unsupported"))
                            ((not (vela-chat--valid-sse-content-type-p headers))
                             (stop)
                             (funcall on-error
                                      "gateway SSE content type is unsupported"))
                            (t
                             (setq headers-complete t
                                   header-probe "")
                             (funcall filter source chunk)))))
                        ((> (string-bytes header-probe)
                            vela-chat-max-http-header-bytes)
                         (stop)
                         (funcall on-error
                                  "gateway stream exceeds header byte bound"))
                        (t (funcall filter source chunk)))))
                    (t (funcall filter source chunk))))))))
          (setq timer (run-at-time 0 vela-chat-sse-poll-seconds #'poll))))
    (list :buffer buffer :timer timer :cancel #'stop))))

(defun vela-chat--protect-region (start end)
  "Make transcript region START through END read-only at its trailing edge."
  (add-text-properties
   start end
   '(read-only t front-sticky (read-only) rear-nonsticky (read-only))))

(defun vela-chat--check-transcript-bound ()
  "Fail when the current transcript exceeds its synchronous UI bound."
  (when (> (buffer-size) vela-chat-max-transcript-characters)
    (signal 'vela-chat-error '("chat transcript exceeds character bound"))))

(defun vela-chat--check-transcript-capacity (delta &optional reserve)
  "Fail before DELTA characters plus optional RESERVE exceed the buffer bound."
  (unless (and (integerp delta) (integerp (or reserve 0)))
    (signal 'vela-chat-error '("chat transcript capacity input is invalid")))
  (when (> (+ (buffer-size) delta (or reserve 0))
           vela-chat-max-transcript-characters)
    (signal 'vela-chat-error '("chat transcript exceeds character bound"))))

(defun vela-chat--append-entry (label text face)
  "Append read-only transcript entry LABEL and TEXT using FACE."
  (vela-chat--check-transcript-capacity
   (+ (length label) 2 (length text) 2)
   (length "You> "))
  (let ((inhibit-read-only t)
        (start (point-max)))
    (goto-char (point-max))
    (insert (propertize (concat label "> ") 'face face) text "\n\n")
    (vela-chat--protect-region start (point))
    (vela-chat--check-transcript-bound)))

(defun vela-chat--append-prompt ()
  "Append one editable composer prompt."
  (vela-chat--check-transcript-capacity (length "You> "))
  (setq buffer-read-only nil)
  (let ((inhibit-read-only t)
        (start (point-max)))
    (goto-char (point-max))
    (insert (propertize "You> " 'face 'vela-chat-human-face))
    (setq vela-chat--input-start (copy-marker (point)))
    (vela-chat--protect-region start (point))
    (goto-char (point-max))))

(defun vela-chat--composer-text ()
  "Return the current editable composer text."
  (unless (and (markerp vela-chat--input-start)
               (marker-buffer vela-chat--input-start))
    (signal 'vela-chat-error '("chat composer is not available")))
  (buffer-substring-no-properties vela-chat--input-start (point-max)))

(defun vela-chat--freeze-composer (message)
  "Replace the current composer with exact normalized MESSAGE and freeze it."
  (let ((inhibit-read-only t)
        (start (marker-position vela-chat--input-start)))
    (vela-chat--check-transcript-capacity
     (- (+ (length message) 2) (- (point-max) start))
     (length "You> "))
    (delete-region start (point-max))
    (goto-char start)
    (insert message "\n\n")
    (vela-chat--protect-region start (point))
    (set-marker vela-chat--input-start nil)
    (setq vela-chat--input-start nil
          buffer-read-only t)))

(defun vela-chat--set-assistant (message-id text)
  "Insert or replace cumulative assistant TEXT identified by MESSAGE-ID."
  (unless (and (stringp message-id) (not (string-empty-p message-id))
               (stringp text))
    (signal 'vela-chat-error '("assistant event has invalid message identity or text")))
  (let ((inhibit-read-only t))
    (if (and (equal message-id vela-chat--assistant-message-id)
             (markerp vela-chat--assistant-start)
             (marker-buffer vela-chat--assistant-start)
             (markerp vela-chat--assistant-end)
             (marker-buffer vela-chat--assistant-end))
        (let ((start (marker-position vela-chat--assistant-start))
              (end (marker-position vela-chat--assistant-end)))
          (vela-chat--check-transcript-capacity
           (- (length text) (- end start)) (length "You> "))
          (delete-region start end)
          (goto-char start)
          (insert text)
          (set-marker vela-chat--assistant-end (point))
          (vela-chat--protect-region start (point)))
      (let ((entry-start (point-max)))
        (vela-chat--check-transcript-capacity
         (+ (length "Assistant> ") (length text) 2) (length "You> "))
        (goto-char (point-max))
        (insert (propertize "Assistant> " 'face 'vela-chat-assistant-face))
        (setq vela-chat--assistant-message-id message-id
              vela-chat--assistant-start (copy-marker (point)))
        (insert text)
        (setq vela-chat--assistant-end (copy-marker (point)))
        (insert "\n\n")
        (vela-chat--protect-region entry-start (point))))
    (vela-chat--check-transcript-bound)))

(defun vela-chat--status-text (payload preferred fallback)
  "Read bounded text from PAYLOAD using PREFERRED or FALLBACK."
  (let ((text (or (vela-chat--field preferred payload)
                  (and fallback (vela-chat--field fallback payload)))))
    (unless (and (stringp text) (not (string-empty-p text)))
      (signal 'vela-chat-error '("gateway event omitted required text")))
    text))

(defun vela-chat--apply-stream-event (event)
  "Apply one decoded gateway stream EVENT to the current transcript."
  (vela-chat--ensure-owner-thread)
  (when vela-chat--terminal
    (signal 'vela-chat-error '("gateway emitted an event after terminal state")))
  (setq vela-chat--event-count (1+ vela-chat--event-count))
  (when (> vela-chat--event-count vela-chat-max-events-per-turn)
    (signal 'vela-chat-error '("gateway stream exceeds event-count bound")))
  (let* ((kind (vela-chat--required-string "kind" event))
         (payload (vela-chat--required-object "payload" event)))
    (pcase kind
      ("thinking"
       (vela-chat--append-entry
        "Thinking" (vela-chat--status-text payload "text" nil)
        'vela-chat-status-face))
      ("tool"
       (let ((name (vela-chat--required-label "toolName" payload))
             (summary (vela-chat--status-text payload "summary" nil)))
         (vela-chat--append-entry (format "Tool · %s" name) summary
                                  'vela-chat-status-face)))
      ((or "assistant" "final")
       (vela-chat--set-assistant
        (vela-chat--required-string "messageId" payload)
        (vela-chat--status-text payload "text" nil))
       (when (string= kind "final")
         (setq vela-chat--terminal t)
         (vela-chat--finish-turn)))
      ("error"
       (setq vela-chat--terminal t)
       (vela-chat--fail-turn
        "Error" (vela-chat--status-text payload "text" nil)))
      ((or "turn.accepted" "session.started" "runtime.status") nil)
      (_ (signal 'vela-chat-error '("unsupported gateway event kind"))))))

(defun vela-chat--decode-stream-event (event)
  "Decode one raw SSE EVENT into the gateway event object."
  (let* ((data (vela-chat--required-string "data" event))
         (decoded (vela-chat--parse-json data))
         (kind-entry (and (listp decoded) (assoc-string "kind" decoded)))
         (kind (and kind-entry (cdr kind-entry)))
         (fallback (vela-chat--field "event" event))
         (explicit-fallback
          (and (stringp fallback)
               (not (string-empty-p fallback))
               (not (string= fallback "message")))))
    (when (and kind-entry
               (not (and (stringp kind) (not (string-empty-p kind)))))
      (signal 'vela-chat-error '("gateway event kind is malformed")))
    (when (and kind-entry explicit-fallback (not (string= kind fallback)))
      (signal 'vela-chat-error '("gateway SSE event identity conflicts with JSON kind")))
    (when (and (not kind-entry) explicit-fallback)
      (setq decoded (cons (cons "kind" fallback) decoded)))
    decoded))

(defun vela-chat--call-cancel (handle)
  "Invoke HANDLE's cancellation closure when present."
  (let ((cancel (plist-get handle :cancel)))
    (when (functionp cancel) (funcall cancel))))

(defun vela-chat--cancel-timeout ()
  "Invalidate and cancel the active lifecycle timeout, when present."
  (setq vela-chat--timeout-serial (1+ vela-chat--timeout-serial))
  (let ((timer vela-chat--timeout-timer))
    (setq vela-chat--timeout-timer nil)
    (when (timerp timer) (cancel-timer timer))))

(defun vela-chat--arm-timeout (generation stage seconds message)
  "Arm a GENERATION- and STAGE-safe timeout after SECONDS with MESSAGE."
  (vela-chat--cancel-timeout)
  (let ((buffer (current-buffer))
        (serial vela-chat--timeout-serial))
    (setq vela-chat--timeout-timer
          (run-at-time
           seconds nil
           (lambda ()
             (when (buffer-live-p buffer)
               (with-current-buffer buffer
                 (when (and vela-chat--busy
                            (= generation vela-chat--generation)
                            (= serial vela-chat--timeout-serial)
                            (eq stage vela-chat--transport-stage))
                   (setq vela-chat--timeout-timer nil)
                   (vela-chat--fail-turn "Timeout" message)))))))))

(defun vela-chat--validate-timeouts ()
  "Reject non-positive, non-numeric, or unbounded timeout configuration."
  (unless (and (numberp vela-chat-operation-timeout-seconds)
               (> vela-chat-operation-timeout-seconds 0)
               (<= vela-chat-operation-timeout-seconds
                   vela-chat-max-timeout-seconds)
               (numberp vela-chat-sse-idle-timeout-seconds)
               (> vela-chat-sse-idle-timeout-seconds 0)
               (<= vela-chat-sse-idle-timeout-seconds
                   vela-chat-max-timeout-seconds))
    (signal 'vela-chat-error
            '("chat timeout values must be positive numbers no greater than 86400"))))

(defun vela-chat--finish-turn ()
  "Complete the current turn and restore an editable prompt."
  (let ((handle vela-chat--active-handle))
    (setq vela-chat--busy nil
          vela-chat--active-handle nil
          vela-chat--transport-stage nil)
    (vela-chat--cancel-timeout)
    (vela-chat--call-cancel handle)
    (vela-chat--append-prompt)))

(defun vela-chat--fail-turn (label message)
  "Fail the current turn with LABEL and bounded MESSAGE."
  (when vela-chat--busy
    (let ((handle vela-chat--active-handle))
      (setq vela-chat--busy nil
            vela-chat--active-handle nil
            vela-chat--transport-stage nil
            vela-chat--terminal t)
      (vela-chat--cancel-timeout)
      (vela-chat--call-cancel handle)
      (condition-case nil
          (vela-chat--append-entry label message 'vela-chat-error-face)
        (vela-chat-error nil))
      (vela-chat--append-prompt))))

(defun vela-chat--guarded (buffer generation function)
  "Return callback invoking FUNCTION in BUFFER for current GENERATION."
  (lambda (&rest arguments)
    (when (buffer-live-p buffer)
      (with-current-buffer buffer
        (when (and vela-chat--busy (= generation vela-chat--generation))
          (condition-case err
              (apply function arguments)
            (error
             (vela-chat--fail-turn "Error" (error-message-string err)))))))))

(defun vela-chat--set-active-handle (generation stage handle)
  "Record HANDLE only while GENERATION and transport STAGE remain active."
  (if (and vela-chat--busy
           (= generation vela-chat--generation)
           (eq stage vela-chat--transport-stage))
      (setq vela-chat--active-handle handle)
    ;; An injected adapter may invoke its callback synchronously and advance the
    ;; lifecycle before returning.  Its now-stale handle must not remain live.
    (vela-chat--call-cancel handle)))

(defun vela-chat--start-stream (generation stream-url)
  "Start SSE STREAM-URL for active GENERATION."
  (setq vela-chat--transport-stage 'stream
        vela-chat--active-handle nil)
  (vela-chat--arm-timeout generation 'stream
                          vela-chat-sse-idle-timeout-seconds
                          "Gateway stream became idle")
  (let* ((buffer (current-buffer))
         (url (vela-chat--resolve-stream-url stream-url))
         (handle
          (funcall
           vela-chat-stream-function
           url
           (vela-chat--guarded
            buffer generation
            (lambda (event)
              (vela-chat--apply-stream-event
               (if (vela-chat--field "data" event)
                   (vela-chat--decode-stream-event event)
                 event))))
           (vela-chat--guarded
            buffer generation
            (lambda ()
              (unless vela-chat--terminal
                (vela-chat--fail-turn
                 "Degraded" "Stream ended before a terminal event"))))
           (vela-chat--guarded
            buffer generation
            (lambda (message)
              (vela-chat--fail-turn "Error" message)))
           (vela-chat--guarded
            buffer generation
            (lambda ()
              (when (eq vela-chat--transport-stage 'stream)
                (vela-chat--arm-timeout
                 generation 'stream vela-chat-sse-idle-timeout-seconds
                 "Gateway stream became idle")))))))
    (vela-chat--set-active-handle generation 'stream handle)))

(defun vela-chat--start-turn (generation session-id message)
  "Submit MESSAGE in SESSION-ID for active GENERATION."
  (setq vela-chat--transport-stage 'turn
        vela-chat--active-handle nil)
  (vela-chat--arm-timeout generation 'turn
                          vela-chat-operation-timeout-seconds
                          "Turn submission timed out")
  (let* ((buffer (current-buffer))
         (url (concat (vela-chat--origin-string) "/api/client/turns"))
         (payload `(("sessionId" . ,session-id)
                    ("input" . (("text" . ,message)))))
         (handle
          (funcall
           vela-chat-post-json-function
           url payload
           (vela-chat--guarded
            buffer generation
            (lambda (response)
              (let* ((turn (vela-chat--required-object "turn" response))
                     (turn-id (vela-chat--required-string "id" turn))
                     (stream-url (vela-chat--required-string "streamUrl" turn)))
                (setq vela-chat--turn-id turn-id)
                (vela-chat--start-stream generation stream-url))))
           (vela-chat--guarded
            buffer generation
            (lambda (message) (vela-chat--fail-turn "Error" message))))))
    (vela-chat--set-active-handle generation 'turn handle)))

(defun vela-chat--start-resolve (generation message)
  "Resolve a session before sending MESSAGE for active GENERATION."
  (setq vela-chat--transport-stage 'resolve
        vela-chat--active-handle nil)
  (vela-chat--arm-timeout generation 'resolve
                          vela-chat-operation-timeout-seconds
                          "Session resolution timed out")
  (let* ((buffer (current-buffer))
         (url (concat (vela-chat--origin-string)
                      "/api/client/sessions/resolve"))
         (payload
          (append
           `(("clientKind" . "emacs")
             ("surfaceId" . "vela-emacs")
             ("sessionMode" . ,vela-chat-session-mode))
           (when vela-chat--session-id
             `(("sessionId" . ,vela-chat--session-id)))))
         (handle
          (funcall
           vela-chat-post-json-function
           url payload
           (vela-chat--guarded
            buffer generation
            (lambda (response)
              (let* ((session (vela-chat--required-object "session" response))
                     (session-id (vela-chat--required-string "id" session))
                     (mode (vela-chat--required-string "mode" session)))
                (unless (string= mode vela-chat-session-mode)
                  (signal 'vela-chat-error
                          '("gateway resolved unexpected session mode")))
                (setq vela-chat--session-id session-id)
                (vela-chat--start-turn generation session-id message))))
           (vela-chat--guarded
            buffer generation
            (lambda (message) (vela-chat--fail-turn "Error" message))))))
    (vela-chat--set-active-handle generation 'resolve handle)))

(defun vela-chat-send ()
  "Send the current composer text through Vela's asynchronous gateway contract."
  (interactive)
  (vela-chat--ensure-owner-thread)
  (when vela-chat--busy
    (signal 'vela-chat-error '("a chat turn is already active")))
  (let ((message (string-trim (vela-chat--composer-text))))
    (when (string-empty-p message)
      (signal 'vela-chat-error '("chat input must not be empty")))
    (when (> (length message) vela-chat-max-input-characters)
      (signal 'vela-chat-error '("chat input exceeds character bound")))
    (unless (member vela-chat-session-mode '("canonical" "isolated"))
      (signal 'vela-chat-error '("chat session mode is invalid")))
    ;; Validate configuration and runtime credential before mutating transcript.
    (vela-chat--origin-string)
    (let ((vela-chat--runtime-token-override
           (cons t (vela-chat--runtime-token))))
      (vela-chat--validate-timeouts)
      (vela-chat--freeze-composer message)
      (setq vela-chat--busy t
            vela-chat--terminal nil
            vela-chat--event-count 0
            vela-chat--turn-id nil
            vela-chat--assistant-message-id nil
            vela-chat--assistant-start nil
            vela-chat--assistant-end nil
            vela-chat--generation (1+ vela-chat--generation))
      (condition-case err
          (vela-chat--start-resolve vela-chat--generation message)
        (error
         (vela-chat--fail-turn "Error" (error-message-string err)))))))

(defun vela-chat-cancel ()
  "Cancel the active chat network operation without clearing session continuity."
  (interactive)
  (vela-chat--ensure-owner-thread)
  (when vela-chat--busy
    (let ((handle vela-chat--active-handle))
      (setq vela-chat--generation (1+ vela-chat--generation)
            vela-chat--busy nil
            vela-chat--active-handle nil
            vela-chat--transport-stage nil
            vela-chat--terminal t)
      (vela-chat--cancel-timeout)
      (vela-chat--call-cancel handle)
      (condition-case nil
          (vela-chat--append-entry
           "Cancelled" "Turn cancelled" 'vela-chat-error-face)
        (vela-chat-error nil))
      (vela-chat--append-prompt))))

(defun vela-chat--cancel-on-kill ()
  "Cancel an active transport while the chat buffer is being killed."
  (when vela-chat--busy
    (let ((handle vela-chat--active-handle))
      (setq vela-chat--generation (1+ vela-chat--generation)
            vela-chat--busy nil
            vela-chat--active-handle nil
            vela-chat--transport-stage nil
            vela-chat--terminal t)
      (vela-chat--cancel-timeout)
      (vela-chat--call-cancel handle))))

(defun vela-chat-new-session ()
  "Clear the live transcript and start a new unresolved gateway session."
  (interactive)
  (vela-chat--ensure-owner-thread)
  (when vela-chat--busy
    (signal 'vela-chat-error '("cancel the active turn before resetting the session")))
  (vela-chat--cancel-timeout)
  (setq vela-chat--session-id nil
        vela-chat--turn-id nil
        vela-chat--terminal nil
        vela-chat--event-count 0
        vela-chat--transport-stage nil
        vela-chat--assistant-message-id nil
        vela-chat--assistant-start nil
        vela-chat--assistant-end nil
        vela-chat--generation (1+ vela-chat--generation))
  (vela-chat--initialize-buffer))

(defun vela-chat--header-line ()
  "Return bounded live chat status for `header-line-format'."
  (format " Vela · %s · session %s"
          (if vela-chat--busy "streaming" "ready")
          (if vela-chat--session-id "active" "new")))

(defun vela-chat--initialize-buffer ()
  "Initialize the current Vela chat transcript and composer."
  (let ((inhibit-read-only t))
    (erase-buffer)
    (insert (propertize "Vela Chat" 'face 'vela-chat-title-face) "\n\n")
    (vela-chat--protect-region (point-min) (point))
    (vela-chat--append-prompt)))

;;;###autoload
(define-derived-mode vela-chat-mode fundamental-mode "Vela-Chat"
  "Major mode for asynchronous chat through Vela's client gateway."
  (setq-local truncate-lines nil)
  (setq-local header-line-format '(:eval (vela-chat--header-line)))
  (setq-local buffer-undo-list t)
  (add-hook 'kill-buffer-hook #'vela-chat--cancel-on-kill nil t)
  (vela-chat--initialize-buffer))

;;;###autoload
(defun vela-chat-open ()
  "Open or display the process-local Vela chat buffer."
  (interactive)
  (let ((buffer (get-buffer-create "*Vela Chat*")))
    (with-current-buffer buffer
      (unless (derived-mode-p 'vela-chat-mode)
        (vela-chat-mode)))
    (pop-to-buffer buffer)))

(provide 'vela-chat-mode)
;;; vela-chat-mode.el ends here
