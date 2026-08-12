;;; vela-chat-mode-test.el --- Tests for Vela gateway chat mode -*- lexical-binding: t; -*-

(require 'ert)
(require 'cl-lib)
(require 'vela-chat-mode)

(defun vela-chat-test--field (name alist)
  "Return string-keyed NAME from ALIST."
  (alist-get name alist nil nil #'string=))

(defmacro vela-chat-test--with-buffer (&rest body)
  "Create a configured chat buffer and evaluate BODY."
  (declare (indent 0) (debug t))
  `(with-temp-buffer
     (let ((vela-chat-base-url "http://127.0.0.1:3847")
           (vela-chat-auth-token-function (lambda () "test-secret")))
       (vela-chat-mode)
       ,@body)))

(ert-deftest vela-chat-mode-keeps-transcript-read-only-and-composer-editable ()
  (vela-chat-test--with-buffer
    (should (derived-mode-p 'vela-chat-mode))
    (should (equal (buffer-substring-no-properties (point-min) (point-max))
                   "Vela Chat\n\nYou> "))
    (should (get-text-property (point-min) 'read-only))
    (goto-char (point-max))
    (insert "hello")
    (should (equal (vela-chat--composer-text) "hello"))
    (goto-char (point-min))
    (should-error (insert "mutate transcript") :type 'text-read-only)))

(ert-deftest vela-chat-composer-can-type-q-and-quit-uses-prefixed-command ()
  (vela-chat-test--with-buffer
    (should (eq (key-binding (kbd "q")) #'self-insert-command))
    (should (eq (key-binding (kbd "C-c C-q")) #'quit-window))
    (let ((last-command-event ?q))
      (call-interactively #'self-insert-command))
    (should (equal (vela-chat--composer-text) "q"))))

(ert-deftest vela-chat-disables-undo-so-protected-transcript-cannot-be-erased ()
  (vela-chat-test--with-buffer
    (goto-char (point-max))
    (insert "hello")
    (let ((before (buffer-string)))
      (should-error (undo 1) :type 'user-error)
      (should (equal (buffer-string) before)))))

(ert-deftest vela-chat-send-resolves-submits-and-reuses-session-exactly ()
  (vela-chat-test--with-buffer
    (let (posts stream-url)
      (setq-local
       vela-chat-post-json-function
       (lambda (url payload on-success _on-error)
         (push (list url payload) posts)
         (if (string-suffix-p "/api/client/sessions/resolve" url)
             (funcall on-success
                      '(("session" . (("id" . "session-1")
                                      ("mode" . "canonical")))))
           (funcall on-success
                    '(("turn" . (("id" . "turn-1")
                                  ("streamUrl" . "/api/client/turns/turn-1/stream"))))))
         '(:cancel ignore))
       vela-chat-stream-function
       (lambda (url on-event on-complete _on-error _on-activity)
         (setq stream-url url)
         (funcall on-event
                  '(("kind" . "final")
                    ("payload" . (("messageId" . "assistant-1")
                                   ("text" . "Hello back")))))
         (funcall on-complete)
         '(:cancel ignore)))
      (goto-char (point-max))
      (insert "  hello Vela  ")
      (vela-chat-send)
      (should (equal (nreverse posts)
                     '(("http://127.0.0.1:3847/api/client/sessions/resolve"
                        (("clientKind" . "emacs")
                         ("surfaceId" . "vela-emacs")
                         ("sessionMode" . "canonical")))
                       ("http://127.0.0.1:3847/api/client/turns"
                        (("sessionId" . "session-1")
                         ("input" . (("text" . "hello Vela"))))))))
      (should (equal stream-url
                     "http://127.0.0.1:3847/api/client/turns/turn-1/stream"))
      (should (equal vela-chat--session-id "session-1"))
      (should-not vela-chat--busy)
      (should (string-match-p "You> hello Vela" (buffer-string)))
      (should (string-match-p "Assistant> Hello back" (buffer-string)))
      (should-not (string-match-p "test-secret" (buffer-string)))
      (setq posts nil)
      (goto-char (point-max))
      (insert "again")
      (vela-chat-send)
      (should (equal
               (vela-chat-test--field
                "sessionId" (cadr (car (last posts))))
               "session-1")))))

(ert-deftest vela-chat-http-body-start-skips-url-header-terminator ()
  (with-temp-buffer
    (insert "headers\nbody")
    (setq-local url-http-end-of-headers (copy-marker 8))
    (should (= (vela-chat--http-body-start) 9))))

(ert-deftest vela-chat-json-response-parsing-normalizes-string-keys ()
  (should
   (equal
    (vela-chat--parse-json
     "{\"session\":{\"id\":\"s1\",\"mode\":\"canonical\"}}")
    '(("session" . (("id" . "s1") ("mode" . "canonical")))))))

(ert-deftest vela-chat-json-requires-canonical-utf8-before-decoding ()
  (let ((canonical
         (encode-coding-string "{\"text\":\"café\"}" 'utf-8 t))
        (crlf
         (encode-coding-string "{\r\n\"text\":\"café\"\r\n}" 'utf-8-unix t)))
    (should (equal (vela-chat--parse-json canonical)
                   '(("text" . "café"))))
    (should (equal (vela-chat--parse-json crlf)
                   '(("text" . "café"))))
    (should (equal (vela-chat--parse-json "{\"text\":\"café\"}")
                   '(("text" . "café")))))
  (dolist (malformed
           (list (concat (string-make-unibyte "{\"text\":\"")
                         (unibyte-string #xc0 #xaf)
                         (string-make-unibyte "\"}"))
                 (concat (string-make-unibyte "{\"text\":\"")
                         (unibyte-string #xed #xa0 #x80)
                         (string-make-unibyte "\"}"))
                 (concat "{\"text\":\"" (string #x3fff80) "\"}")
                 (concat "{\"text\":\"" (string #x110000) "\"}")))
    (should-error (vela-chat--parse-json malformed) :type 'vela-chat-error)))

(ert-deftest vela-chat-required-object-rejects-json-null ()
  (should-error
   (vela-chat--required-object
    "session" (vela-chat--parse-json "{\"session\":null}"))
   :type 'vela-chat-error))

(ert-deftest vela-chat-required-object-rejects-absent-field ()
  (should-error
   (vela-chat--required-object "session" '(("other" . ())))
   :type 'vela-chat-error))

(ert-deftest vela-chat-required-object-accepts-present-empty-object ()
  (should-not (vela-chat--required-object "session" '(("session" . nil)))))

(ert-deftest vela-chat-json-request-encoding-preserves-string-keyed-contract ()
  (should
   (equal
    (decode-coding-string
     (vela-chat--encode-json-request
      '(("clientKind" . "emacs")
        ("input" . (("text" . "hello")))))
     'utf-8)
    "{\"clientKind\":\"emacs\",\"input\":{\"text\":\"hello\"}}")))

(ert-deftest vela-chat-sse-parser-supports-fragments-crlf-and-multiline-data ()
  (let ((parser (vela-chat--sse-parser-create)))
    (should (equal (vela-chat--sse-feed parser "event: assistant\r\nda" nil) []))
    (should
     (equal
      (vela-chat--sse-feed
       parser
       "ta: {\"kind\":\"assistant\",\r\ndata: \"payload\":{\"text\":\"hi\"}}\r\n\r\n"
       nil)
      [(("event" . "assistant")
        ("data" . "{\"kind\":\"assistant\",\n\"payload\":{\"text\":\"hi\"}}"))]))
    (should (equal (vela-chat--sse-feed parser "" t) []))))

(ert-deftest vela-chat-sse-parser-discards-unterminated-event-at-eof ()
  (let ((parser (vela-chat--sse-parser-create)))
    (should
     (equal
      (vela-chat--sse-feed
       parser
       "event: final\ndata: {\"kind\":\"final\",\"payload\":{}}"
       t)
      []))
    (should (string-empty-p (vela-chat--sse-parser-pending parser)))
    (should-not (vela-chat--sse-parser-event parser))
    (should-not (vela-chat--sse-parser-data-lines parser))
    (should (= (vela-chat--sse-parser-data-characters parser) 0))))

(ert-deftest vela-chat-sse-parser-supports-lone-cr-line-endings ()
  (let ((parser (vela-chat--sse-parser-create)))
    (should
     (equal
      (vela-chat--sse-feed
       parser
       "event: final\rdata: {\"kind\":\"final\",\"payload\":{}}\r\r"
       nil)
      [(("event" . "final")
        ("data" . "{\"kind\":\"final\",\"payload\":{}}"))]))))

(ert-deftest vela-chat-sse-parser-treats-split-crlf-as-one-line-ending ()
  (let ((parser (vela-chat--sse-parser-create)))
    (should (equal (vela-chat--sse-feed parser "event: final\r" nil) []))
    (should
     (equal
      (vela-chat--sse-feed
       parser
       "\ndata: {\"kind\":\"final\",\"payload\":{}}\r\n\r\n"
       nil)
      [(("event" . "final")
        ("data" . "{\"kind\":\"final\",\"payload\":{}}"))]))))

(ert-deftest vela-chat-sse-parser-dispatches-before-split-crlf-lf-arrives ()
  (let ((parser (vela-chat--sse-parser-create)))
    (should
     (equal (vela-chat--sse-feed parser "data: payload\r\r" nil)
            [(("event" . "message") ("data" . "payload"))]))
    (should (equal (vela-chat--sse-feed parser "\n" nil) []))))

(ert-deftest vela-chat-sse-parser-preserves-lf-after-complete-crlf-chunk ()
  (let ((parser (vela-chat--sse-parser-create)))
    (should (equal (vela-chat--sse-feed parser "data: payload\r\n" nil) []))
    (should
     (equal (vela-chat--sse-feed parser "\n" nil)
            [(("event" . "message") ("data" . "payload"))]))))

(ert-deftest vela-chat-sse-event-field-removes-only-one-leading-space ()
  (dolist (case '(("event:  final" . " final")
                  ("event:\tfinal" . "\tfinal")))
    (let* ((parser (vela-chat--sse-parser-create))
           (events
            (vela-chat--sse-feed
             parser
             (concat (car case) "\n"
                     "data: {\"kind\":\"final\",\"payload\":{}}\n\n")
             nil)))
      (should (= (length events) 1))
      (should (equal (vela-chat--field "event" (aref events 0)) (cdr case)))
      (should-error
       (vela-chat--decode-stream-event (aref events 0))
       :type 'vela-chat-error))))

(ert-deftest vela-chat-sse-validates-complete-lines-as-canonical-utf8 ()
  (let* ((parser (vela-chat--sse-parser-create))
         (bytes
          (encode-coding-string
           "data: {\"kind\":\"thinking\",\"payload\":{\"text\":\"café\"}}\n\n"
           'utf-8 t))
         (split (string-match (unibyte-string #xc3) bytes)))
    (should (equal (vela-chat--sse-feed parser (substring bytes 0 (1+ split)) nil)
                   []))
    (let* ((events (vela-chat--sse-feed parser (substring bytes (1+ split)) nil))
           (decoded (vela-chat--decode-stream-event (aref events 0))))
      (should (equal (vela-chat--field
                      "text" (vela-chat--required-object "payload" decoded))
                     "café"))))
  (let ((parser (vela-chat--sse-parser-create)))
    (should-error
     (vela-chat--sse-feed
      parser (concat "ignored: " (unibyte-string #xc0 #xaf) "\n\n") nil)
     :type 'vela-chat-error)))

(ert-deftest vela-chat-sse-event-identities-must-be-coherent ()
  (should
   (equal
    (vela-chat--decode-stream-event
     '(("event" . "assistant")
       ("data" . "{\"kind\":\"assistant\",\"payload\":{}}")))
    '(("kind" . "assistant") ("payload"))))
  (should
   (equal
    (vela-chat--field
     "kind"
     (vela-chat--decode-stream-event
      '(("event" . "assistant")
        ("data" . "{\"payload\":{}}"))))
    "assistant"))
  (should-error
   (vela-chat--decode-stream-event
    '(("event" . "tool")
      ("data" . "{\"kind\":\"assistant\",\"payload\":{}}")))
   :type 'vela-chat-error)
  (dolist (kind '("null" "false" "\"\""))
    (should-error
     (vela-chat--decode-stream-event
      `(("event" . "assistant")
        ("data" . ,(format "{\"kind\":%s,\"payload\":{}}" kind))))
     :type 'vela-chat-error)))

(ert-deftest vela-chat-sse-parser-enforces-total-stream-byte-bound ()
  (let ((parser (vela-chat--sse-parser-create)))
    (should-error
     (vela-chat--sse-feed
      parser (make-string (1+ vela-chat-max-sse-response-bytes) ?x) nil)
     :type 'vela-chat-error)))

(ert-deftest vela-chat-sse-parser-bounds-events-before-building-a-batch ()
  (let ((parser (vela-chat--sse-parser-create))
        (event "data: {\"kind\":\"runtime.status\",\"payload\":{}}\n\n"))
    (should-error
     (vela-chat--sse-feed
      parser
      (apply #'concat
             (make-list (1+ vela-chat-max-events-per-turn) event))
      nil)
     :type 'vela-chat-error)
    (should (= (vela-chat--sse-parser-event-count parser)
               vela-chat-max-events-per-turn))))

(ert-deftest vela-chat-origin-rendering-preserves-only-explicit-ports ()
  (let ((vela-chat-base-url "http://127.0.0.1"))
    (should (equal (vela-chat--origin-string) "http://127.0.0.1")))
  (let ((vela-chat-base-url "http://127.0.0.1:80"))
    (should (equal (vela-chat--origin-string) "http://127.0.0.1:80"))))

(ert-deftest vela-chat-sse-content-type-accepts-case-and-valid-parameters ()
  (dolist (headers
           '("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n"
             "HTTP/1.1 200 OK\r\nCONTENT-TYPE: TEXT/EVENT-STREAM; charset=utf-8\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=\"utf-8\"; profile=live\r\n\r\n"))
    (should (vela-chat--valid-sse-content-type-p headers))))

(ert-deftest vela-chat-sse-content-type-rejects-missing-malformed-and-duplicate ()
  (dolist (headers
           (list
            "HTTP/1.1 200 OK\r\nServer: test\r\n\r\n"
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n"
            "HTTP/1.1 200 OK\r\nContent-Type : text/event-stream\r\n\r\n"
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset\r\n\r\n"
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=\"utf-8\r\n\r\n"
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Type: text/event-stream\r\n\r\n"
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Type: text/plain\r\n\r\n"
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Type : text/plain\r\n\r\n"
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n , text/plain\r\n\r\n"
            (concat "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; note=\""
                    (string 0) "\"\r\n\r\n")
            (concat "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; note=\""
                    (string 127) "\"\r\n\r\n")
            (concat "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; note=\"\\"
                    (string 1) "\"\r\n\r\n")))
    (should-not (vela-chat--valid-sse-content-type-p headers))))

(ert-deftest vela-chat-json-content-type-accepts-case-and-valid-parameters ()
  (dolist (headers
           '("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n"
             "HTTP/1.1 200 OK\r\nCONTENT-TYPE: APPLICATION/JSON; charset=utf-8\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type: application/json; profile=\"gateway\"\r\n\r\n"))
    (should (vela-chat--valid-content-type-p headers "application/json"))))

(ert-deftest vela-chat-json-content-type-rejects-missing-malformed-and-duplicate ()
  (dolist (headers
           '("HTTP/1.1 200 OK\r\nServer: test\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type : application/json\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset =utf-8\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset= utf-8\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Type: application/json\r\n\r\n"
             "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n text/plain\r\n\r\n"))
    (should-not (vela-chat--valid-content-type-p headers "application/json"))))

(ert-deftest vela-chat-assistant-events-replace-cumulative-message-by-id ()
  (vela-chat-test--with-buffer
    (setq-local vela-chat--busy t)
    (vela-chat--apply-stream-event
     '(("kind" . "assistant")
       ("payload" . (("messageId" . "m1") ("text" . "Hel")))))
    (vela-chat--apply-stream-event
     '(("kind" . "assistant")
       ("payload" . (("messageId" . "m1") ("text" . "Hello")))))
    (should (= (how-many "Assistant>" (point-min) (point-max)) 1))
    (should (string-match-p "Assistant> Hello" (buffer-string)))
    (should (string-suffix-p "Assistant> Hello\n\n" (buffer-string)))
    (goto-char (point-min))
    (search-forward "Hello")
    (should (get-text-property (1- (point)) 'read-only))
    (should-error (insert "mutate assistant") :type 'text-read-only)
    (should-not (string-match-p "Assistant> Hel\\(?:\n\\|$\\)" (buffer-string)))))

(ert-deftest vela-chat-rejects-cross-origin-and-credentialed-stream-urls ()
  (vela-chat-test--with-buffer
    (should-error
     (vela-chat--resolve-stream-url "https://evil.example/turns/1")
     :type 'vela-chat-error)
    (should-error
     (vela-chat--resolve-stream-url "http://user@127.0.0.1:3847/turns/1")
     :type 'vela-chat-error)
    (should-error
     (vela-chat--resolve-stream-url "//evil.example/turns/1")
     :type 'vela-chat-error)
    (dolist (unsafe
             (list "/turns/1\r\nX-Evil: yes"
                   "/turns/1\\@evil.example"
                   "http://127.0.0.1:3847/turns/1#fragment"
                   (concat "/turns/1" (string 0) "suffix")))
      (should-error (vela-chat--resolve-stream-url unsafe)
                    :type 'vela-chat-error))
    (let ((vela-chat-base-url "http://127.0.0.1:3847\r\nX-Evil: yes"))
      (should-error (vela-chat--origin-string) :type 'vela-chat-error))
    (should (equal
             (vela-chat--resolve-stream-url "/turns/1?cursor=2")
             "http://127.0.0.1:3847/turns/1?cursor=2"))))

(ert-deftest vela-chat-cancel-invokes-transport-and-ignores-late-callbacks ()
  (vela-chat-test--with-buffer
    (let (resolve-success cancelled)
      (setq-local
       vela-chat-post-json-function
       (lambda (_url _payload on-success _on-error)
         (setq resolve-success on-success)
         (list :cancel (lambda () (setq cancelled t)))))
      (goto-char (point-max))
      (insert "wait")
      (vela-chat-send)
      (should vela-chat--busy)
      (vela-chat-cancel)
      (should cancelled)
      (should-not vela-chat--busy)
      (should-not vela-chat--timeout-timer)
      (funcall resolve-success
               '(("session" . (("id" . "late") ("mode" . "canonical")))))
      (should-not vela-chat--session-id)
      (should (string-match-p "Cancelled> Turn cancelled" (buffer-string))))))

(ert-deftest vela-chat-final-event-cancels-the-live-stream-handle ()
  (vela-chat-test--with-buffer
    (let (stream-event cancelled)
      (setq-local
       vela-chat-post-json-function
       (lambda (url _payload on-success _on-error)
         (if (string-suffix-p "/sessions/resolve" url)
             (funcall on-success
                      '(("session" . (("id" . "s") ("mode" . "canonical")))))
           (funcall on-success
                    '(("turn" . (("id" . "t") ("streamUrl" . "/stream"))))))
         '(:cancel ignore))
       vela-chat-stream-function
       (lambda (_url on-event _on-complete _on-error _on-activity)
         (setq stream-event on-event)
         (list :cancel (lambda () (setq cancelled t)))))
      (goto-char (point-max))
      (insert "finish")
      (vela-chat-send)
      (should vela-chat--busy)
      (funcall stream-event
               '(("kind" . "final")
                 ("payload" . (("messageId" . "m") ("text" . "done")))))
      (should cancelled)
      (should-not vela-chat--busy))))

(ert-deftest vela-chat-operation-timeout-cancels-stalled-resolve-and-recovers ()
  (vela-chat-test--with-buffer
    (let (timeout-callback transport-cancelled cancelled-timers)
      (cl-letf (((symbol-function 'run-at-time)
                 (lambda (_delay _repeat function &rest arguments)
                   (setq timeout-callback
                         (lambda () (apply function arguments)))
                   'fake-timer))
                ((symbol-function 'timerp) (lambda (value) (eq value 'fake-timer)))
                ((symbol-function 'cancel-timer)
                 (lambda (timer) (push timer cancelled-timers))))
        (setq-local
         vela-chat-post-json-function
         (lambda (_url _payload _on-success _on-error)
           (list :cancel (lambda () (setq transport-cancelled t)))))
        (goto-char (point-max))
        (insert "stall")
        (vela-chat-send)
        (should (functionp timeout-callback))
        (funcall timeout-callback)
        (should transport-cancelled)
        (should-not vela-chat--busy)
        (should (string-match-p "Timeout> Session resolution timed out"
                                (buffer-string)))
        (goto-char (point-max))
        (insert "retry")
        (should (equal (vela-chat--composer-text) "retry"))))))

(ert-deftest vela-chat-turn-operation-timeout-cancels-stalled-submit ()
  (vela-chat-test--with-buffer
    (let (timeout-callback transport-cancelled)
      (cl-letf (((symbol-function 'run-at-time)
                 (lambda (_delay _repeat function &rest arguments)
                   (setq timeout-callback
                         (lambda () (apply function arguments)))
                   'fake-timer))
                ((symbol-function 'timerp) (lambda (value) (eq value 'fake-timer)))
                ((symbol-function 'cancel-timer) #'ignore))
        (setq-local
         vela-chat-post-json-function
         (lambda (url _payload on-success _on-error)
           (if (string-suffix-p "/sessions/resolve" url)
               (funcall on-success
                        '(("session" . (("id" . "s") ("mode" . "canonical")))))
             (setq transport-cancelled nil))
           (list :cancel (lambda () (setq transport-cancelled t)))))
        (goto-char (point-max))
        (insert "stall submit")
        (vela-chat-send)
        (should (eq vela-chat--transport-stage 'turn))
        (funcall timeout-callback)
        (should transport-cancelled)
        (should-not vela-chat--busy)
        (should (string-match-p "Timeout> Turn submission timed out"
                                (buffer-string)))))))

(ert-deftest vela-chat-rejects-unbounded-timeouts-before-freezing-composer ()
  (dolist (settings
           `((,(read "1.0e+INF") 60)
             (,(1+ vela-chat-max-timeout-seconds) 60)
             (30 ,(1+ vela-chat-max-timeout-seconds))))
    (vela-chat-test--with-buffer
      (let ((vela-chat-operation-timeout-seconds (car settings))
            (vela-chat-sse-idle-timeout-seconds (cadr settings))
            called)
        (setq-local vela-chat-post-json-function
                    (lambda (&rest _) (setq called t)))
        (goto-char (point-max))
        (insert "remain editable")
        (should-error (vela-chat-send) :type 'vela-chat-error)
        (should-not called)
        (should-not vela-chat--busy)
        (should (equal (vela-chat--composer-text) "remain editable"))))))

(ert-deftest vela-chat-stream-activity-rearms-idle-timeout-and-stale-timer-is-inert ()
  (vela-chat-test--with-buffer
    (let (activity timeout-callbacks cancelled-timers)
      (cl-letf (((symbol-function 'run-at-time)
                 (lambda (_delay _repeat function &rest arguments)
                   (let ((callback (lambda () (apply function arguments))))
                     (push callback timeout-callbacks)
                     callback)))
                ((symbol-function 'timerp) #'functionp)
                ((symbol-function 'cancel-timer)
                 (lambda (timer) (push timer cancelled-timers))))
        (setq-local
         vela-chat-post-json-function
         (lambda (url _payload on-success _on-error)
           (funcall on-success
                    (if (string-suffix-p "/sessions/resolve" url)
                        '(("session" . (("id" . "s") ("mode" . "canonical"))))
                      '(("turn" . (("id" . "t") ("streamUrl" . "/stream"))))))
           '(:cancel ignore))
         vela-chat-stream-function
         (lambda (_url _on-event _on-complete _on-error on-activity)
           (setq activity on-activity)
           '(:cancel ignore)))
        (goto-char (point-max))
        (insert "stream")
        (vela-chat-send)
        (should (eq vela-chat--transport-stage 'stream))
        (let ((first-idle-timer (car timeout-callbacks)))
          (funcall activity)
          (should (= (length timeout-callbacks) 4))
          (should (member first-idle-timer cancelled-timers))
          (funcall first-idle-timer)
          (should vela-chat--busy)
          (funcall (car timeout-callbacks))
          (should-not vela-chat--busy)
          (should (string-match-p "Timeout> Gateway stream became idle"
                                  (buffer-string))))))))

(ert-deftest vela-chat-terminal-completion-cancels-timeout-and-stale-callback-is-inert ()
  (vela-chat-test--with-buffer
    (let (stream-event timeout-callbacks cancelled-timers)
      (cl-letf (((symbol-function 'run-at-time)
                 (lambda (_delay _repeat function &rest arguments)
                   (let ((callback (lambda () (apply function arguments))))
                     (push callback timeout-callbacks)
                     callback)))
                ((symbol-function 'timerp) #'functionp)
                ((symbol-function 'cancel-timer)
                 (lambda (timer) (push timer cancelled-timers))))
        (setq-local
         vela-chat-post-json-function
         (lambda (url _payload on-success _on-error)
           (funcall on-success
                    (if (string-suffix-p "/sessions/resolve" url)
                        '(("session" . (("id" . "s") ("mode" . "canonical"))))
                      '(("turn" . (("id" . "t") ("streamUrl" . "/stream"))))))
           '(:cancel ignore))
         vela-chat-stream-function
         (lambda (_url on-event _on-complete _on-error _on-activity)
           (setq stream-event on-event)
           '(:cancel ignore)))
        (goto-char (point-max))
        (insert "finish")
        (vela-chat-send)
        (let ((idle-timer (car timeout-callbacks)))
          (funcall stream-event
                   '(("kind" . "final")
                     ("payload" . (("messageId" . "m") ("text" . "done")))))
          (should-not vela-chat--busy)
          (should (member idle-timer cancelled-timers))
          (funcall idle-timer)
          (should-not vela-chat--busy)
          (should-not (string-match-p "Timeout>" (buffer-string))))))))

(ert-deftest vela-chat-synchronous-stage-handles-are-cancelled-as-stale ()
  (vela-chat-test--with-buffer
    (let (cancelled)
      (setq-local
       vela-chat-post-json-function
       (lambda (url _payload on-success _on-error)
         (funcall on-success
                  (if (string-suffix-p "/sessions/resolve" url)
                      '(("session" . (("id" . "s") ("mode" . "canonical"))))
                    '(("turn" . (("id" . "t") ("streamUrl" . "/stream"))))))
         (let ((stage (if (string-suffix-p "/sessions/resolve" url)
                          'resolve
                        'turn)))
           (list :cancel (lambda () (push stage cancelled)))))
       vela-chat-stream-function
       (lambda (_url _on-event _on-complete _on-error _on-activity)
         '(:cancel ignore)))
      (goto-char (point-max))
      (insert "synchronous")
      (vela-chat-send)
      (should (equal cancelled '(resolve turn)))
      (should vela-chat--busy)
      (vela-chat-cancel))))

(ert-deftest vela-chat-killing-an-active-buffer-cancels-transport ()
  (let ((buffer (generate-new-buffer " *vela-chat-kill-test*")) cancelled)
    (unwind-protect
        (progn
          (with-current-buffer buffer
            (let ((vela-chat-base-url "http://127.0.0.1:3847")
                  (vela-chat-auth-token-function nil))
              (vela-chat-mode)
              (setq-local
               vela-chat-post-json-function
               (lambda (_url _payload _on-success _on-error)
                 (list :cancel (lambda () (setq cancelled t)))))
              (goto-char (point-max))
              (insert "wait")
              (vela-chat-send)))
          (kill-buffer buffer)
          (should cancelled))
      (when (buffer-live-p buffer) (kill-buffer buffer)))))

(ert-deftest vela-chat-active-turn-protects-the-trailing-transcript-boundary ()
  (vela-chat-test--with-buffer
    (setq-local
     vela-chat-post-json-function
     (lambda (_url _payload _on-success _on-error) '(:cancel ignore)))
    (goto-char (point-max))
    (insert "pending")
    (vela-chat-send)
    (should vela-chat--busy)
    (goto-char (point-max))
    (should-error (insert "INJECTED") :type 'buffer-read-only)
    (vela-chat-cancel)))

(ert-deftest vela-chat-transcript-overflow-recovers-within-bound ()
  (vela-chat-test--with-buffer
    (let ((inhibit-read-only t)
          (event `(("kind" . "thinking")
                   ("payload" . (("text" . ,(make-string 100 ?y))))))
          cancelled)
      (goto-char (point-max))
      (insert (make-string
               (- vela-chat-max-transcript-characters (buffer-size) 10)
               ?x))
      (vela-chat--protect-region (point-min) (point-max))
      (setq-local vela-chat--busy t
                  vela-chat--generation 1
                  vela-chat--active-handle
                  (list :cancel (lambda () (setq cancelled t))))
      (funcall
       (vela-chat--guarded
        (current-buffer) 1
        (lambda () (vela-chat--apply-stream-event event))))
      (should cancelled)
      (should-not vela-chat--busy)
      (should (<= (buffer-size) vela-chat-max-transcript-characters))
      (should (markerp vela-chat--input-start)))))

(ert-deftest vela-chat-rejects-resolved-session-mode-mismatch ()
  (vela-chat-test--with-buffer
    (let (urls)
      (setq-local
       vela-chat-post-json-function
       (lambda (url _payload on-success _on-error)
         (push url urls)
         (funcall on-success
                  '(("session" . (("id" . "s") ("mode" . "ephemeral")))))
         '(:cancel ignore)))
      (goto-char (point-max))
      (insert "mode")
      (vela-chat-send)
      (should (= (length urls) 1))
      (should-not vela-chat--busy)
      (should-not vela-chat--session-id)
      (should (string-match-p "resolved unexpected session mode" (buffer-string))))))

(ert-deftest vela-chat-synchronous-start-error-restores-composer ()
  (vela-chat-test--with-buffer
    (setq-local vela-chat-post-json-function
                (lambda (&rest _) (error "synchronous adapter failure")))
    (goto-char (point-max))
    (insert "recover")
    (vela-chat-send)
    (should-not vela-chat--busy)
    (should (markerp vela-chat--input-start))
    (should (string-match-p "Error> synchronous adapter failure" (buffer-string)))
    (goto-char (point-max))
    (insert "next")
    (should (equal (vela-chat--composer-text) "next"))))

(ert-deftest vela-chat-second-runtime-token-failure-restores-composer ()
  (vela-chat-test--with-buffer
    (let ((calls 0))
      (setq-local
       vela-chat-auth-token-function
       (lambda ()
         (setq calls (1+ calls))
         (if (= calls 1)
             "first-token"
           (error "token backend transient failure"))))
      (goto-char (point-max))
      (insert "hello")
      (vela-chat-send)
      (should (= calls 2))
      (should-not vela-chat--busy)
      (should (markerp vela-chat--input-start))
      (should (string-match-p "Error> token backend transient failure"
                              (buffer-string))))))

(ert-deftest vela-chat-new-session-resets-state-and-rejects-busy-reset ()
  (vela-chat-test--with-buffer
    (setq-local vela-chat--session-id "session"
                vela-chat--turn-id "turn"
                vela-chat--assistant-message-id "message")
    (vela-chat-new-session)
    (should-not vela-chat--timeout-timer)
    (should-not vela-chat--session-id)
    (should-not vela-chat--turn-id)
    (should-not vela-chat--assistant-message-id)
    (should (equal (buffer-string) "Vela Chat\n\nYou> "))
    (setq-local vela-chat--session-id "active"
                vela-chat--busy t)
    (should-error (vela-chat-new-session) :type 'vela-chat-error)
    (should (equal vela-chat--session-id "active"))
    (should vela-chat--busy)
    (setq-local vela-chat--busy nil)))

(ert-deftest vela-chat-rejects-oversized-composer-before-transport ()
  (vela-chat-test--with-buffer
    (let ((called nil))
      (setq-local vela-chat-post-json-function
                  (lambda (&rest _) (setq called t)))
      (goto-char (point-max))
      (insert (make-string (1+ vela-chat-max-input-characters) ?x))
      (should-error (vela-chat-send) :type 'vela-chat-error)
      (should-not called)
      (should-not vela-chat--busy))))

(ert-deftest vela-chat-nonterminal-stream-completion-is-degraded ()
  (vela-chat-test--with-buffer
    (let (complete)
      (setq-local
       vela-chat-post-json-function
       (lambda (url _payload on-success _on-error)
         (if (string-suffix-p "/sessions/resolve" url)
             (funcall on-success
                      '(("session" . (("id" . "s") ("mode" . "canonical")))))
           (funcall on-success
                    '(("turn" . (("id" . "t") ("streamUrl" . "/stream"))))))
         '(:cancel ignore))
       vela-chat-stream-function
       (lambda (_url on-event on-complete _on-error _on-activity)
         (funcall on-event
                  '(("kind" . "assistant")
                    ("payload" . (("messageId" . "m")
                                   ("text" . "partial")))))
         (setq complete on-complete)
         '(:cancel ignore)))
      (goto-char (point-max))
      (insert "go")
      (vela-chat-send)
      (funcall complete)
      (should-not vela-chat--busy)
      (should (string-match-p "Degraded> Stream ended before a terminal event"
                              (buffer-string))))))

(ert-deftest vela-chat-unterminated-final-stream-completion-is-degraded ()
  (vela-chat-test--with-buffer
    (let ((parser (vela-chat--sse-parser-create)))
      (setq-local
       vela-chat-post-json-function
       (lambda (url _payload on-success _on-error)
         (if (string-suffix-p "/sessions/resolve" url)
             (funcall on-success
                      '(("session" . (("id" . "s") ("mode" . "canonical")))))
           (funcall on-success
                    '(("turn" . (("id" . "t") ("streamUrl" . "/stream"))))))
         '(:cancel ignore))
       vela-chat-stream-function
       (lambda (_url on-event on-complete _on-error _on-activity)
         (mapc on-event
               (append
                (vela-chat--sse-feed
                 parser
                 "event: final\ndata: {\"kind\":\"final\",\"payload\":{\"messageId\":\"m\",\"text\":\"incomplete\"}}"
                 t)
                nil))
         (funcall on-complete)
         '(:cancel ignore)))
      (goto-char (point-max))
      (insert "go")
      (vela-chat-send)
      (should-not vela-chat--busy)
      (should-not (string-match-p "incomplete" (buffer-string)))
      (should (string-match-p "Degraded> Stream ended before a terminal event"
                              (buffer-string))))))

(ert-deftest vela-chat-http-retrieval-rejects-oversized-transport-incrementally ()
  (let (clients result)
    (cl-labels
        ((filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               ;; Never terminate the response headers: this exercises the raw
               ;; process-filter cap before URL parsing can discover a body.
               (process-send-string
                process
                (concat
                 "HTTP/1.1 200 OK\r\nX-Oversized: "
                 (make-string
                  (+ vela-chat-max-http-header-bytes
                     vela-chat-max-http-response-bytes 1)
                  ?x))))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-oversized-gateway"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (vela-chat--url-post-json
               (format "http://127.0.0.1:%d/oversized"
                       (process-contact server :service))
               '(("probe" . t))
               (lambda (_) (setq result 'unexpected-success))
               (lambda (message) (setq result message)))
              (let ((deadline (+ (float-time) 5.0)))
                (while (and (not result) (< (float-time) deadline))
                  (accept-process-output nil 0.05)))
              (should (string-match-p "byte bound" result)))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-http-rejects-non-json-content-type-before-dispatch ()
  (let (clients success result)
    (cl-labels
        ((filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               (let ((body "{\"session\":{\"id\":\"forged\",\"mode\":\"canonical\"}}"))
                 (process-send-string
                  process
                  (format (concat "HTTP/1.1 200 OK\r\n"
                                  "Content-Type: text/plain\r\n"
                                  "Content-Length: %d\r\n"
                                  "Connection: close\r\n\r\n%s")
                          (string-bytes body) body))
                 (process-send-eof process)))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-invalid-json-content-type"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (vela-chat--url-post-json
               (format "http://127.0.0.1:%d/json"
                       (process-contact server :service))
               '(("probe" . t))
               (lambda (_) (setq success t result 'unexpected-success))
               (lambda (message) (setq result message)))
              (let ((deadline (+ (float-time) 5.0)))
                (while (and (not result) (< (float-time) deadline))
                  (accept-process-output nil 0.05)))
              (should-not success)
              (should (equal result "gateway JSON content type is unsupported")))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-http-retrieval-rejects-oversized-headers-incrementally ()
  (let (clients result)
    (cl-labels
        ((filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               (process-send-string
                process
                (concat "HTTP/1.1 200 OK\r\nX-Fill: "
                        (make-string (1+ vela-chat-max-http-header-bytes) ?x)))
               (process-send-eof process))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-oversized-http-header"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (vela-chat--url-post-json
               (format "http://127.0.0.1:%d/oversized-header"
                       (process-contact server :service))
               '(("probe" . t))
               (lambda (_) (setq result 'unexpected-success))
               (lambda (message) (setq result message)))
              (let ((deadline (+ (float-time) 5.0)))
                (while (and (not result) (< (float-time) deadline))
                  (accept-process-output nil 0.05)))
              (should (string-match-p "header byte bound" result)))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-sse-retrieval-rejects-oversized-headers-incrementally ()
  (let (clients result)
    (cl-labels
        ((filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               (process-send-string
                process
                (concat "HTTP/1.1 200 OK\r\nX-Fill: "
                        (make-string (1+ vela-chat-max-http-header-bytes) ?x)))
               (process-send-eof process))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-oversized-sse-header"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (vela-chat--url-stream
               (format "http://127.0.0.1:%d/stream"
                       (process-contact server :service))
               (lambda (_) (setq result 'unexpected-event))
               (lambda () (setq result 'unexpected-completion))
               (lambda (message) (setq result message)))
              (let ((deadline (+ (float-time) 5.0)))
                (while (and (not result) (< (float-time) deadline))
                  (accept-process-output nil 0.05)))
              (should (string-match-p "header byte bound" result)))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-sse-retrieval-tracks-decoder-buffer-mutations ()
  (let (clients events result handle)
    (cl-labels
        ((send-chunk (process text)
           (process-send-string
            process (format "%x\r\n%s\r\n" (string-bytes text) text)))
         (filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               (process-send-string
                process
                (concat "HTTP/1.1 200 OK\r\n"
                        "Content-Type: text/event-stream\r\n"
                        "Transfer-Encoding: chunked\r\n"
                        "Connection: close\r\n\r\n"))
               (send-chunk process "event: primer\ndata: ready\n\n"))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-chunked-sse"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (setq handle
                    (vela-chat--url-stream
                     (format "http://127.0.0.1:%d/stream"
                             (process-contact server :service))
                     (lambda (event)
                       (push event events)
                       (when (= (length events) 1)
                         ;; The primer callback runs only after poll advanced its
                         ;; cursor.  Model url-http removing transfer framing
                         ;; before that consumed point between polling passes.
                         (let ((retrieval (plist-get handle :buffer)))
                           (with-current-buffer retrieval
                             (let ((inhibit-read-only t)
                                   (start (vela-chat--http-body-start)))
                               (delete-region start (1+ start)))))
                         (let ((process (car clients)))
                           (send-chunk
                            process
                            "event: final\ndata: {\"kind\":\"final\",\"payload\":{\"messageId\":\"m\",\"text\":\"chunked\"}}\n\n")
                           (process-send-string process "0\r\n\r\n")
                           (process-send-eof process))))
                     (lambda () (setq result 'complete))
                     (lambda (message) (setq result (list 'error message)))))
              (let ((deadline (+ (float-time) 5.0)))
                (while (and (not result) (< (float-time) deadline))
                  (accept-process-output nil 0.05)))
              (should (eq result 'complete))
              (should (= (length events) 2))
              (should (string-match-p "chunked"
                                      (vela-chat-test--field
                                       "data" (car events)))))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-sse-event-cancellation-stops-poll-without-error ()
  (let (clients events errors finished handle)
    (cl-labels
        ((filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               (let ((body
                      (concat "event: final\ndata: done\n\n"
                              "event: ignored\ndata: after-cancel\n\n")))
                 (process-send-string
                  process
                  (format (concat "HTTP/1.1 200 OK\r\n"
                                  "Content-Type: text/event-stream\r\n"
                                  "Content-Length: %d\r\n"
                                  "Connection: close\r\n\r\n%s")
                          (string-bytes body) body))
                 (process-send-eof process)))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-cancel-on-event"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (setq handle
                    (vela-chat--url-stream
                     (format "http://127.0.0.1:%d/stream"
                             (process-contact server :service))
                     (lambda (event)
                       (push event events)
                       (vela-chat--call-cancel handle)
                       (setq finished t))
                     (lambda () (setq finished 'unexpected-completion))
                     (lambda (message)
                       (push message errors)
                       (setq finished t))))
              (let ((deadline (+ (float-time) 5.0)))
                (while (and (not finished) (< (float-time) deadline))
                  (accept-process-output nil 0.05)))
              (should (eq finished t))
              (should-not errors)
              (should (= (length events) 1)))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-sse-rejects-non-success-status-before-dispatch ()
  (let (clients events result)
    (cl-labels
        ((filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               (process-send-string
                process
                (concat
                 "HTTP/1.1 302 Found\r\n"
                 "Content-Type: text/event-stream\r\n"
                 "Connection: keep-alive\r\n\r\n"
                 "event: final\n"
                 "data: {\"kind\":\"final\",\"payload\":{\"messageId\":\"m\",\"text\":\"forged\"}}\n\n")))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-non-success-sse"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (vela-chat--url-stream
               (format "http://127.0.0.1:%d/stream"
                       (process-contact server :service))
               (lambda (event) (push event events))
               (lambda () (setq result 'unexpected-completion))
               (lambda (message) (setq result message)))
              (let ((deadline (+ (float-time) 5.0)))
                (while (and (not result) (< (float-time) deadline))
                  (accept-process-output nil 0.05)))
              (should-not events)
              (should (string-match-p "HTTP 302" result)))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-sse-rejects-non-event-stream-content-type-before-dispatch ()
  (let (clients events result)
    (cl-labels
        ((filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               (let ((body
                      "event: final\ndata: {\"kind\":\"final\",\"payload\":{\"messageId\":\"m\",\"text\":\"forged\"}}\n\n"))
                 (process-send-string
                  process
                  (format (concat "HTTP/1.1 200 OK\r\n"
                                  "Content-Type: text/plain\r\n"
                                  " , text/event-stream\r\n"
                                  "Content-Length: %d\r\n"
                                  "Connection: close\r\n\r\n%s")
                          (string-bytes body) body))
                 (process-send-eof process)))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-invalid-sse-content-type"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (vela-chat--url-stream
               (format "http://127.0.0.1:%d/stream"
                       (process-contact server :service))
               (lambda (event) (push event events))
               (lambda () (setq result 'unexpected-completion))
               (lambda (message) (setq result message)))
              (let ((deadline (+ (float-time) 5.0)))
                (while (and (not result) (< (float-time) deadline))
                  (accept-process-output nil 0.05)))
              (should-not events)
              (should (equal result "gateway SSE content type is unsupported")))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-sse-rejects-content-encoding-before-decoder ()
  (let (clients events result handle)
    (cl-labels
        ((filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match-p "\r\n\r\n" request)
               (set-process-filter process #'ignore)
               (process-send-string
                process
                (concat
                 "HTTP/1.1 200 OK\r\n"
                 "Content-Type: text/event-stream\r\n"
                 "Content-Encoding: gzip\r\n"
                 "Connection: keep-alive\r\n\r\n"
                 "data: {\"kind\":\"final\",\"payload\":{\"messageId\":\"m\",\"text\":\"forged\"}}\n\n")))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-content-encoding-sse"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (let ((vela-chat-auth-token-function nil))
              (setq handle
                    (vela-chat--url-stream
                     (format "http://127.0.0.1:%d/stream"
                             (process-contact server :service))
                     (lambda (event) (push event events))
                     (lambda () (setq result 'unexpected-completion))
                     (lambda (message) (setq result message))))
              (let ((deadline (+ (float-time) 2.0)))
                (while (and (not result) (< (float-time) deadline))
                  (accept-process-output nil 0.02)))
              (should-not events)
              (should (equal result
                             "gateway content encoding is unsupported")))
          (when handle (vela-chat--call-cancel handle))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(ert-deftest vela-chat-real-async-http-and-sse-round-trip ()
  (let (clients requests)
    (cl-labels
        ((respond (process content-type body)
           (process-send-string
            process
            (format (concat "HTTP/1.1 200 OK\r\n"
                            "Content-Type: %s\r\n"
                            "Content-Length: %d\r\n"
                            "Connection: close\r\n\r\n%s")
                    content-type (string-bytes body) body))
           (process-send-eof process))
         (filter (process chunk)
           (let ((request (concat (or (process-get process 'request) "") chunk)))
             (process-put process 'request request)
             (when (string-match "\r\n\r\n" request)
               (let* ((header-end (match-end 0))
                      (case-fold-search t)
                      (content-length
                       (if (string-match "\r\nContent-Length: \\([0-9]+\\)" request)
                           (string-to-number (match-string 1 request))
                         0)))
                 (when (>= (string-bytes (substring request header-end))
                           content-length)
                   (push request requests)
                   (cond
                    ((string-match-p "POST /api/client/sessions/resolve " request)
                     (respond process "application/json"
                              "{\"session\":{\"id\":\"mock-session\",\"mode\":\"canonical\"}}"))
                    ((string-match-p "POST /api/client/turns " request)
                     (respond process "application/json"
                              "{\"turn\":{\"id\":\"mock-turn\",\"streamUrl\":\"/mock-stream\"}}"))
                    ((string-match-p "GET /mock-stream " request)
                     (respond
                      process "text/event-stream"
                      (concat
                       "event: final\n"
                       "data: {\"kind\":\"final\",\"payload\":"
                       "{\"messageId\":\"mock-message\",\"text\":\"Async hello\"}}\n\n")))
                    (t
                     (process-send-string process
                                          "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                     (process-send-eof process))))))))
         (log-client (_server client _message)
           (push client clients)
           (set-process-query-on-exit-flag client nil)
           (set-process-filter client #'filter)))
      (let ((server
             (make-network-process
              :name "vela-chat-test-gateway"
              :server t :host "127.0.0.1" :service t :family 'ipv4
              :noquery t :log #'log-client)))
        (unwind-protect
            (vela-chat-test--with-buffer
              (let ((vela-chat-base-url
                     (format "http://127.0.0.1:%d"
                             (process-contact server :service))))
                (goto-char (point-max))
                (insert "round trip")
                (vela-chat-send)
                (let ((deadline (+ (float-time) 5.0)))
                  (while (and vela-chat--busy (< (float-time) deadline))
                    (accept-process-output nil 0.05))))
              (should-not vela-chat--busy)
              (should (equal vela-chat--session-id "mock-session"))
              (should (string-match-p "Assistant> Async hello" (buffer-string)))
              (should (= (length requests) 3))
              (should-not (string-match-p "test-secret" (buffer-string))))
          (dolist (client clients)
            (when (process-live-p client) (delete-process client)))
          (when (process-live-p server) (delete-process server)))))))

(provide 'vela-chat-mode-test)
;;; vela-chat-mode-test.el ends here
