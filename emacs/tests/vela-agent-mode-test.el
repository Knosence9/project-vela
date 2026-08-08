;;; vela-agent-mode-test.el --- Tests for Vela's Emacs agent interface -*- lexical-binding: t; -*-

(require 'ert)
(require 'vela-agent-mode)

(ert-deftest vela-agent-capabilities-are-stable-and-read-only ()
  (let* ((response (vela-agent-handle-request
                    '(("operation" . "capabilities.list"))))
         (result (alist-get "result" response nil nil #'string=))
         (capabilities (alist-get "capabilities" result nil nil #'string=))
         (features (alist-get "emacs_features" result nil nil #'string=)))
    (should (equal (alist-get "protocol_version" response nil nil #'string=) 1))
    (should (eq (alist-get "ok" response nil nil #'string=) t))
    (should
     (equal capabilities
            [(("name" . "capabilities.list")
              ("effect" . "read"))
             (("name" . "context.snapshot")
              ("effect" . "read"))]))
    (should (equal (mapcar (lambda (feature)
                             (alist-get "name" feature nil nil #'string=))
                           (append features nil))
                   '("buffer" "org" "project" "diagnostics" "compilation" "magit")))
    (dolist (feature (append features nil))
      (should (memq (alist-get "available" feature nil nil #'string=)
                    '(t :false)))
      (should (equal (alist-get "threading" feature nil nil #'string=)
                     "main-thread-snapshot")))))

(ert-deftest vela-agent-capability-discovery-does-not-search-load-path ()
  (cl-letf (((symbol-function 'locate-library)
             (lambda (&rest _)
               (ert-fail "capability discovery searched load-path"))))
    (should (eq
             (alist-get
              "ok"
              (vela-agent-handle-request
               '(("operation" . "capabilities.list")))
              nil nil #'string=)
             t))))

(ert-deftest vela-agent-capabilities-identify-exposed-context-sections ()
  (let* ((response (vela-agent-handle-request
                    '(("operation" . "capabilities.list"))))
         (result (alist-get "result" response nil nil #'string=))
         (features (alist-get "emacs_features" result nil nil #'string=)))
    (should (equal (mapcar (lambda (feature)
                             (alist-get "context_section" feature nil nil #'string=))
                           (append features nil))
                   '("buffer" "org" :null :null :null :null)))))

(ert-deftest vela-agent-context-snapshot-reports-buffer-without-mutating-it ()
  (with-temp-buffer
    (rename-buffer " *vela-agent-test*")
    (insert "alpha\nbeta\n")
    (text-mode)
    (goto-char 7)
    (set-buffer-modified-p nil)
    (let* ((point-before (point))
           (response
            (vela-agent-handle-request
             '(("operation" . "context.snapshot")
               ("include" . ["buffer"]))))
           (result (alist-get "result" response nil nil #'string=))
           (buffer (alist-get "buffer" result nil nil #'string=)))
      (should (eq (alist-get "ok" response nil nil #'string=) t))
      (should (equal buffer
                     '(("name" . " *vela-agent-test*")
                       ("file" . :null)
                       ("major_mode" . "text-mode")
                       ("modified" . :false)
                       ("point" . 7)
                       ("line" . 2)
                       ("column" . 0)
                       ("region" . :null))))
      (should (= (point) point-before))
      (should-not (buffer-modified-p)))))

(ert-deftest vela-agent-context-snapshot-uses-native-org-context ()
  (with-temp-buffer
    (org-mode)
    (insert "* TODO Build interface :emacs:\n"
            ":PROPERTIES:\n:ID: vela-heading-1\n:END:\n"
            "#+name: sample-block\n"
            "#+begin_src emacs-lisp\n(+ 1 2)\n#+end_src\n")
    (goto-char (point-min))
    (search-forward "(+ 1 2)")
    (let* ((point-before (point))
           (text-before (buffer-string))
           (modified-before (buffer-modified-p))
           (tick-before (buffer-chars-modified-tick))
           (mark-before (mark t))
           (mark-active-before mark-active)
           (narrowed-before (buffer-narrowed-p))
           (undo-before buffer-undo-list)
           (match-before (progn
                           (string-match "b\\(c\\)" "abcd")
                           (match-data t)))
           (response
            (vela-agent-handle-request
             '(("operation" . "context.snapshot")
               ("include" . ["org"]))))
           (result (alist-get "result" response nil nil #'string=))
           (org-context (alist-get "org" result nil nil #'string=))
           (heading (alist-get "heading" org-context nil nil #'string=))
           (block (alist-get "source_block" org-context nil nil #'string=)))
      (should (equal heading
                     '(("id" . "vela-heading-1")
                       ("title" . "Build interface")
                       ("level" . 1)
                       ("todo" . "TODO")
                       ("tags" . ["emacs"])
                       ("outline_path" . ["Build interface"]))))
      (should (equal (alist-get "name" block nil nil #'string=)
                     "sample-block"))
      (should (equal (alist-get "language" block nil nil #'string=)
                     "emacs-lisp"))
      (should (equal (alist-get "source_sha256" block nil nil #'string=)
                     (secure-hash 'sha256 "(+ 1 2)")))
      (should (= (point) point-before))
      (should (equal (buffer-string) text-before))
      (should (eq (buffer-modified-p) modified-before))
      (should (= (buffer-chars-modified-tick) tick-before))
      (should (equal (mark t) mark-before))
      (should (eq mark-active mark-active-before))
      (should (eq (buffer-narrowed-p) narrowed-before))
      (should (equal buffer-undo-list undo-before))
      (should (equal (match-data t) match-before)))))

(ert-deftest vela-agent-interface-json-preserves-protocol-order ()
  (let* ((json
          (vela-agent-encode-response
           '(("protocol_version" . 1)
             ("ok" . t)
             ("result" . (("missing" . :null)
                            ("enabled" . :false)
                            ("items" . ["a" "b"]))))))
         (parsed (json-parse-string json
                                    :object-type 'alist
                                    :array-type 'array
                                    :null-object :null
                                    :false-object :false)))
    (should
     (equal json
            "{\"protocol_version\":1,\"ok\":true,\"result\":{\"missing\":null,\"enabled\":false,\"items\":[\"a\",\"b\"]}}"))
    (should (eq (alist-get "missing"
                           (alist-get "result" parsed nil nil #'string=)
                           nil nil #'string=)
                :null))
    (should (eq (alist-get "enabled"
                           (alist-get "result" parsed nil nil #'string=)
                           nil nil #'string=)
                :false))))

(ert-deftest vela-agent-json-encoding-rejects-cycles-and-oversized-values ()
  (let ((cycle (list '("value" . t))))
    (setcdr cycle cycle)
    (should-error (vela-agent-encode-response cycle)
                  :type 'vela-agent-protocol-error))
  (should-error
   (vela-agent-encode-response
    `(("value" . ,(make-string (1+ vela-agent-max-json-string-characters)
                                ?x))))
   :type 'vela-agent-protocol-error)
  (should-error
   (vela-agent-encode-response
    (make-vector (1+ vela-agent-max-json-collection-items) t))
   :type 'vela-agent-protocol-error)
  (let ((nested t))
    (dotimes (_ (1+ vela-agent-max-json-depth))
      (setq nested (vector nested)))
    (should-error (vela-agent-encode-response nested)
                  :type 'vela-agent-protocol-error))
  (let ((many-nodes
         (make-vector vela-agent-max-json-collection-items
                      (vector t t t t))))
    (should-error (vela-agent-encode-response many-nodes)
                  :type 'vela-agent-protocol-error))
  (let ((large-output
         (make-vector 40
                      (make-string vela-agent-max-json-string-characters ?x))))
    (should-error (vela-agent-encode-response large-output)
                  :type 'vela-agent-protocol-error)))

(ert-deftest vela-agent-interface-mode-renders-the-source-context ()
  (with-temp-buffer
    (rename-buffer " *vela-agent-source*")
    (insert "durable context")
    (text-mode)
    (let ((interface (vela-agent-interface-open)))
      (unwind-protect
          (with-current-buffer interface
            (should (eq major-mode 'vela-agent-interface-mode))
            (should buffer-read-only)
            (should (string-match-p
                     "context\\.snapshot"
                     (buffer-substring-no-properties (point-min) (point-max))))
            (should (string-match-p
                     "vela-agent-source"
                     (buffer-substring-no-properties (point-min) (point-max)))))
        (kill-buffer interface)))))

(ert-deftest vela-agent-unsupported-operation-fails-closed ()
  (should-error
   (vela-agent-handle-request
    '(("operation" . "emacs.eval")
      ("form" . "(delete-file dangerous-path)")))
   :type 'vela-agent-protocol-error))

(ert-deftest vela-agent-malformed-request-fails-with-protocol-error ()
  (should-error
   (vela-agent-handle-request "not-an-object")
   :type 'vela-agent-protocol-error))

(ert-deftest vela-agent-dispatch-rejects-worker-thread-editor-access ()
  (let* ((worker
          (make-thread
           (lambda ()
             (condition-case error-data
                 (progn
                   (vela-agent-handle-request
                    '(("operation" . "capabilities.list")))
                   'unexpected-success)
               (error error-data)))))
         (result (thread-join worker)))
    (should (eq (car result) 'vela-agent-protocol-error))))

(ert-deftest vela-agent-context-snapshot-rejects-unknown-sections ()
  (should-error
   (vela-agent-handle-request
    '(("operation" . "context.snapshot")
      ("include" . ["buffer" "secrets"])))
   :type 'vela-agent-protocol-error))

(ert-deftest vela-agent-context-snapshot-bounds-sections-before-copying ()
  (cl-letf (((symbol-function 'append)
             (lambda (&rest _)
               (error "include vector was copied before its size was checked"))))
    (should-error
     (vela-agent-handle-request
      '(("operation" . "context.snapshot")
        ("include" . ["buffer" "org" "buffer"])))
     :type 'vela-agent-protocol-error)))

(ert-deftest vela-agent-request-validation-bounds-cyclic-objects ()
  (let ((request (list '("operation" . "capabilities.list"))))
    (setcdr request request)
    (should-error (vela-agent-handle-request request)
                  :type 'vela-agent-protocol-error)))

(ert-deftest vela-agent-request-validation-bounds-object-fields ()
  (let ((request (cons '("operation" . "capabilities.list")
                       (mapcar (lambda (number)
                                 (cons (format "extra-%d" number) t))
                               (number-sequence 1 8)))))
    (should-error (vela-agent-handle-request request)
                  :type 'vela-agent-protocol-error)))

(ert-deftest vela-agent-context-snapshot-rejects-oversized-buffers ()
  (with-temp-buffer
    (insert (make-string (1+ vela-agent-max-buffer-characters) ?x))
    (should-error
     (vela-agent-handle-request
      '(("operation" . "context.snapshot")
        ("include" . ["buffer"])))
     :type 'vela-agent-protocol-error)))

(ert-deftest vela-agent-context-snapshot-rejects-oversized-buffer-metadata ()
  (with-temp-buffer
    (setq buffer-file-name
          (make-string (1+ vela-agent-max-metadata-string-characters) ?x))
    (should-error
     (vela-agent-handle-request
      '(("operation" . "context.snapshot")
        ("include" . ["buffer"])))
     :type 'vela-agent-protocol-error)))

;;; vela-agent-mode-test.el ends here
