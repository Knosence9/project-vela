;;; vela-agent-mode-test.el --- Tests for Vela's Emacs agent interface -*- lexical-binding: t; -*-

(require 'ert)
(require 'vela-agent-mode)
(require 'flymake)

(ert-deftest vela-agent-capabilities-are-stable-and-read-only ()
  (let* ((response (vela-agent-handle-request
                    '(("operation" . "capabilities.list"))))
         (result (alist-get "result" response nil nil #'string=))
         (capabilities (alist-get "capabilities" result nil nil #'string=))
         (features (alist-get "emacs_features" result nil nil #'string=)))
    (should (equal (alist-get "protocol_version" response nil nil #'string=) 6))
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
                   '("buffer" "org" "project" "diagnostics" :null :null)))))

(ert-deftest vela-agent-context-snapshot-reports-current-line-flymake-diagnostics ()
  (with-temp-buffer
    (insert "alpha\nbeta\ngamma\n")
    (goto-char 8)
    (narrow-to-region 2 17)
    (set-mark 10)
    (setq mark-active t)
    (set-buffer-modified-p nil)
    (string-match "b\\(c\\)" "abcd")
    (let* ((point-before (point))
           (mark-before (mark t))
           (mark-active-before mark-active)
           (restriction-before (cons (point-min) (point-max)))
           (text-before (save-restriction
                          (widen)
                          (buffer-substring (point-min) (point-max))))
           (modified-before (buffer-modified-p))
           (tick-before (buffer-chars-modified-tick))
           (undo-before buffer-undo-list)
           (match-before (match-data t))
           (warning (flymake-make-diagnostic
                     (current-buffer) 8 10 :warning "later"))
           (error (flymake-make-diagnostic
                   (current-buffer) 7 8 :error "first"))
           requested-range)
      (cl-letf (((symbol-function 'flymake-diagnostics)
                 (lambda (beg end)
                   (setq requested-range (cons beg end))
                   (list warning error))))
        (let* ((response
                (vela-agent-handle-request
                 '(("operation" . "context.snapshot")
                   ("include" . ["diagnostics"]))))
               (result (alist-get "result" response nil nil #'string=)))
          (should (equal requested-range '(7 . 12)))
          (should
           (equal result
                  '(("diagnostics" .
                     [(("start" . 7)
                       ("end" . 8)
                       ("type" . "error")
                       ("text" . "first"))
                      (("start" . 8)
                       ("end" . 10)
                       ("type" . "warning")
                       ("text" . "later"))]))))))
      (should (= (point) point-before))
      (should (equal (mark t) mark-before))
      (should (eq mark-active mark-active-before))
      (should (equal (cons (point-min) (point-max)) restriction-before))
      (should
       (equal-including-properties
        (save-restriction
          (widen)
          (buffer-substring (point-min) (point-max)))
        text-before))
      (should (eq (buffer-modified-p) modified-before))
      (should (= (buffer-chars-modified-tick) tick-before))
      (should (equal buffer-undo-list undo-before))
      (should (equal (match-data t) match-before)))))

(ert-deftest vela-agent-context-snapshot-reports-empty-flymake-diagnostics ()
  (with-temp-buffer
    (cl-letf (((symbol-function 'flymake-diagnostics)
               (lambda (&rest _) nil)))
      (let* ((response
              (vela-agent-handle-request
               '(("operation" . "context.snapshot")
                 ("include" . ["diagnostics"]))))
             (result (alist-get "result" response nil nil #'string=)))
        (should (equal result '(("diagnostics" . []))))))))

(ert-deftest vela-agent-context-snapshot-orders-all-flymake-diagnostic-fields ()
  (with-temp-buffer
    (insert "line")
    (let ((diagnostics
           (list
            (flymake-make-diagnostic (current-buffer) 1 3 :warning "z")
            (flymake-make-diagnostic (current-buffer) 1 3 :warning "a")
            (flymake-make-diagnostic (current-buffer) 1 3 :error "z")
            (flymake-make-diagnostic (current-buffer) 1 2 :warning "z"))))
      (cl-letf (((symbol-function 'flymake-diagnostics)
                 (lambda (&rest _) diagnostics)))
        (let* ((items
                (alist-get
                 "diagnostics"
                 (alist-get
                  "result"
                  (vela-agent-handle-request
                   '(("operation" . "context.snapshot")
                     ("include" . ["diagnostics"])))
                  nil nil #'string=)
                 nil nil #'string=))
               (keys
                (mapcar
                 (lambda (item)
                   (list (alist-get "end" item nil nil #'string=)
                         (alist-get "type" item nil nil #'string=)
                         (alist-get "text" item nil nil #'string=)))
                 (append items nil))))
          (should (equal keys
                         '((2 "warning" "z")
                           (3 "error" "z")
                           (3 "warning" "a")
                           (3 "warning" "z")))))))))

(ert-deftest vela-agent-context-snapshot-rejects-zero-width-flymake-diagnostics ()
  (with-temp-buffer
    (insert "line")
    (dolist (position '(1 5))
      (goto-char position)
      (let ((diagnostic
             (flymake-make-diagnostic
              (current-buffer) position position :note "point")))
        (cl-letf (((symbol-function 'flymake-diagnostics)
                   (lambda (&rest _) (list diagnostic))))
          (should-error
           (vela-agent-handle-request
            '(("operation" . "context.snapshot")
              ("include" . ["diagnostics"])))
           :type 'vela-agent-protocol-error))))))

(ert-deftest vela-agent-context-snapshot-bounds-flymake-diagnostic-count ()
  (with-temp-buffer
    (insert "x")
    (let ((diagnostic
           (flymake-make-diagnostic (current-buffer) 1 2 :note "note")))
      (cl-letf (((symbol-function 'flymake-diagnostics)
                 (lambda (&rest _)
                   (make-list (1+ vela-agent-max-json-collection-items)
                              diagnostic))))
        (should-error
         (vela-agent-handle-request
          '(("operation" . "context.snapshot")
            ("include" . ["diagnostics"])))
         :type 'vela-agent-protocol-error)))))

(ert-deftest vela-agent-json-encoding-accepts-complete-bounded-snapshot ()
  (with-temp-buffer
    (insert "x")
    (let* ((text-length
            (- (/ vela-agent-max-diagnostics-json-characters
                  vela-agent-max-json-collection-items)
               80))
           (diagnostic
            (flymake-make-diagnostic
             (current-buffer) 1 2 :note (make-string text-length ?x)))
           (diagnostics
            (make-list vela-agent-max-json-collection-items diagnostic)))
      (cl-letf (((symbol-function 'flymake-diagnostics)
                 (lambda (&rest _) diagnostics))
                ((symbol-function 'project-current) (lambda (&rest _) nil)))
        (let ((response
               (vela-agent-handle-request
                '(("operation" . "context.snapshot")
                  ("include" . ["buffer" "org" "project" "diagnostics"])))))
          (should (stringp (vela-agent-encode-response response))))))))

(ert-deftest vela-agent-context-snapshot-bounds-aggregate-diagnostic-json ()
  (with-temp-buffer
    (insert "x")
    (let* ((diagnostic
            (flymake-make-diagnostic
             (current-buffer) 1 2 :note
             (make-string vela-agent-max-metadata-string-characters ?x)))
           (diagnostics
            (make-list vela-agent-max-json-collection-items diagnostic)))
      (cl-letf (((symbol-function 'flymake-diagnostics)
                 (lambda (&rest _) diagnostics)))
        (should-error
         (vela-agent-handle-request
          '(("operation" . "context.snapshot")
            ("include" . ["diagnostics"])))
         :type 'vela-agent-protocol-error)))))

(ert-deftest vela-agent-context-snapshot-counts-diagnostic-array-separators ()
  (with-temp-buffer
    (insert "x")
    (let* ((empty-diagnostic
            (flymake-make-diagnostic (current-buffer) 1 2 :note ""))
           (empty-item
            (vela-agent--diagnostic-context-item empty-diagnostic 1 2))
           (empty-item-characters
            (length
             (vela-agent--json-serialize
              empty-item 0 (make-hash-table :test #'eq) (vector 0))))
           (item-budget
            (/ vela-agent-max-diagnostics-json-characters
               vela-agent-max-json-collection-items))
           (diagnostic
            (flymake-make-diagnostic
             (current-buffer) 1 2 :note
             (make-string (- item-budget empty-item-characters) ?x)))
           (diagnostics
            (make-list vela-agent-max-json-collection-items diagnostic)))
      (cl-letf (((symbol-function 'flymake-diagnostics)
                 (lambda (&rest _) diagnostics)))
        (should-error
         (vela-agent-handle-request
          '(("operation" . "context.snapshot")
            ("include" . ["diagnostics"])))
         :type 'vela-agent-protocol-error)))))

(ert-deftest vela-agent-context-snapshot-rejects-invalid-flymake-metadata ()
  (with-temp-buffer
    (insert "line\n")
    (let* ((other-buffer (generate-new-buffer " *vela-other-diagnostic*"))
           (foreign-marker (set-marker (make-marker) 1 other-buffer))
           (unset-marker (make-marker)))
      (unwind-protect
          (dolist (fields `((,other-buffer 1 2 :error "text")
                            (,(current-buffer) 0 2 :error "text")
                            (,(current-buffer) 3 2 :error "text")
                            (,(current-buffer) 1 7 :error "text")
                            (,(current-buffer) ,foreign-marker 2 :error "text")
                            (,(current-buffer) ,unset-marker 2 :error "text")
                            (,(current-buffer) 1 2 "error" "text")
                            (,(current-buffer) 1 2 :error
                             ,(make-string
                               (1+ vela-agent-max-metadata-string-characters)
                               ?x))))
            (cl-letf (((symbol-function 'flymake-diagnostic-buffer)
                       (lambda (_) (nth 0 fields)))
                      ((symbol-function 'flymake-diagnostic-beg)
                       (lambda (_) (nth 1 fields)))
                      ((symbol-function 'flymake-diagnostic-end)
                       (lambda (_) (nth 2 fields)))
                      ((symbol-function 'flymake-diagnostic-type)
                       (lambda (_) (nth 3 fields)))
                      ((symbol-function 'flymake-diagnostic-text)
                       (lambda (_) (nth 4 fields))))
              (should-error
               (vela-agent--diagnostic-context-item 'diagnostic 1 6)
               :type 'vela-agent-protocol-error)))
        (kill-buffer other-buffer)))))

(ert-deftest vela-agent-context-snapshot-rejects-improper-flymake-results ()
  (with-temp-buffer
    (insert "line")
    (cl-letf (((symbol-function 'flymake-diagnostics)
               (lambda (&rest _) (cons 'diagnostic 'improper))))
      (cl-letf (((symbol-function 'vela-agent--diagnostic-context-item)
                 (lambda (&rest _) '(("start" . 1)))))
        (should-error
         (vela-agent--diagnostics-context)
         :type 'vela-agent-protocol-error)))))

(ert-deftest vela-agent-context-snapshot-reports-native-project-root-read-only ()
  (with-temp-buffer
    (insert "alpha\nbeta\n")
    (goto-char 7)
    (narrow-to-region 2 10)
    (set-mark 8)
    (setq mark-active t)
    (set-buffer-modified-p nil)
    (string-match "b\\(c\\)" "abcd")
    (let ((point-before (point))
          (mark-before (mark t))
          (mark-active-before mark-active)
          (restriction-before (cons (point-min) (point-max)))
          (text-before (save-restriction
                         (widen)
                         (buffer-substring (point-min) (point-max))))
          (modified-before (buffer-modified-p))
          (tick-before (buffer-chars-modified-tick))
          (undo-before buffer-undo-list)
          (match-before (match-data t))
          (project-object '(vela-test-project)))
      (cl-letf (((symbol-function 'project-current)
                 (lambda (&optional _maybe-prompt _directory) project-object))
                ((symbol-function 'project-root)
                 (lambda (project)
                   (should (eq project project-object))
                   (string-match "changed" "backend-changed-match-data")
                   "/tmp/vela-project/")))
        (let* ((response
                (vela-agent-handle-request
                 '(("operation" . "context.snapshot")
                   ("include" . ["project"]))))
               (result (alist-get "result" response nil nil #'string=)))
          (should (equal result
                         '(("project" . (("root" . "/tmp/vela-project/"))))))))
      (should (= (point) point-before))
      (should (equal (mark t) mark-before))
      (should (eq mark-active mark-active-before))
      (should (equal (cons (point-min) (point-max)) restriction-before))
      (should
       (equal-including-properties
        (save-restriction
          (widen)
          (buffer-substring (point-min) (point-max)))
        text-before))
      (should (eq (buffer-modified-p) modified-before))
      (should (= (buffer-chars-modified-tick) tick-before))
      (should (equal buffer-undo-list undo-before))
      (should (equal (match-data t) match-before)))))

(ert-deftest vela-agent-context-snapshot-reports-missing-project-as-null ()
  (cl-letf (((symbol-function 'project-current) (lambda (&rest _) nil))
            ((symbol-function 'project-root)
             (lambda (&rest _) (ert-fail "missing project resolved a root"))))
    (let* ((response
            (vela-agent-handle-request
             '(("operation" . "context.snapshot")
               ("include" . ["project"]))))
           (result (alist-get "result" response nil nil #'string=)))
      (should (equal result '(("project" . :null)))))))

(ert-deftest vela-agent-context-snapshot-rejects-oversized-project-root ()
  (cl-letf (((symbol-function 'project-current) (lambda (&rest _) 'project))
            ((symbol-function 'project-root)
             (lambda (_project)
               (make-string (1+ vela-agent-max-metadata-string-characters) ?x))))
    (should-error
     (vela-agent-handle-request
      '(("operation" . "context.snapshot")
        ("include" . ["project"])))
     :type 'vela-agent-protocol-error)))

(ert-deftest vela-agent-context-snapshot-rejects-invalid-project-roots ()
  (dolist (root '("relative/project/" 42))
    (cl-letf (((symbol-function 'project-current) (lambda (&rest _) 'project))
              ((symbol-function 'project-root) (lambda (_project) root)))
      (should-error
       (vela-agent-handle-request
        '(("operation" . "context.snapshot")
          ("include" . ["project"])))
       :type 'vela-agent-protocol-error))))

(ert-deftest vela-agent-context-snapshot-uses-project-api-dispatch ()
  (let ((default-directory temporary-file-directory)
        (project-find-functions
         (list (lambda (_directory) `(transient . ,temporary-file-directory)))))
    (let* ((response
            (vela-agent-handle-request
             '(("operation" . "context.snapshot")
               ("include" . ["project"]))))
           (project (alist-get
                     "project"
                     (alist-get "result" response nil nil #'string=)
                     nil nil #'string=)))
      (should (equal (alist-get "root" project nil nil #'string=)
                     temporary-file-directory)))))

(ert-deftest vela-agent-context-snapshot-reports-buffer-without-mutating-it ()
  (with-temp-buffer
    (rename-buffer " *vela-agent-test*")
    (insert "alpha\nbeta\n")
    (text-mode)
    (goto-char 7)
    (set-buffer-modified-p nil)
    (let* ((point-before (point))
           (tick-before (buffer-chars-modified-tick))
           (response
            (vela-agent-handle-request
             '(("operation" . "context.snapshot")
               ("include" . ["buffer"]))))
           (result (alist-get "result" response nil nil #'string=))
           (buffer (alist-get "buffer" result nil nil #'string=))
           (identity (alist-get "identity" buffer nil nil #'string=)))
      (should (eq (alist-get "ok" response nil nil #'string=) t))
      (should (stringp identity))
      (should (equal buffer
                     `(("name" . " *vela-agent-test*")
                       ("file" . :null)
                       ("identity" . ,identity)
                       ("major_mode" . "text-mode")
                       ("modified" . :false)
                       ("point" . 7)
                       ("line" . 2)
                       ("column" . 0)
                       ("region" . :null)
                       ("text_revision" . ,tick-before)
                       ("restriction" . (("start" . 1)
                                          ("end" . 12)
                                          ("narrowed" . :false))))))
      (should (= (point) point-before))
      (should-not (buffer-modified-p)))))

(ert-deftest vela-agent-context-snapshot-reports-narrowing-without-widening ()
  (with-temp-buffer
    (insert "zero\nalpha\nomega\n")
    (text-mode)
    (put-text-property 7 10 'vela-agent-test-property t)
    (narrow-to-region 6 12)
    (goto-char 8)
    (set-mark 10)
    (setq mark-active t)
    (set-buffer-modified-p nil)
    (string-match "b\\(c\\)" "abcd")
    (let* ((point-before (point))
           (mark-before (mark t))
           (mark-active-before mark-active)
           (restriction-before (cons (point-min) (point-max)))
           (text-before (save-restriction
                          (widen)
                          (buffer-substring (point-min) (point-max))))
           (modified-before (buffer-modified-p))
           (tick-before (buffer-chars-modified-tick))
           (undo-before buffer-undo-list)
           (match-before (match-data t))
           (response
            (vela-agent-handle-request
             '(("operation" . "context.snapshot")
               ("include" . ["buffer"]))))
           (result (alist-get "result" response nil nil #'string=))
           (buffer (alist-get "buffer" result nil nil #'string=)))
      (should (equal (alist-get "restriction" buffer nil nil #'string=)
                     '(("start" . 6)
                       ("end" . 12)
                       ("narrowed" . t))))
      (should (equal (cons (point-min) (point-max)) restriction-before))
      (should (= (point) point-before))
      (should (equal (mark t) mark-before))
      (should (eq mark-active mark-active-before))
      (should
       (equal-including-properties
        (save-restriction
          (widen)
          (buffer-substring (point-min) (point-max)))
        text-before))
      (should (eq (buffer-modified-p) modified-before))
      (should (= (buffer-chars-modified-tick) tick-before))
      (should (equal buffer-undo-list undo-before))
      (should (equal (match-data t) match-before)))))

(ert-deftest vela-agent-context-snapshot-reports-character-revision ()
  (with-temp-buffer
    (insert "alpha")
    (let* ((request '(("operation" . "context.snapshot")
                      ("include" . ["buffer"])))
           (revision
            (lambda ()
              (alist-get
               "text_revision"
               (alist-get
                "buffer"
                (alist-get
                 "result" (vela-agent-handle-request request)
                 nil nil #'string=)
                nil nil #'string=)
               nil nil #'string=)))
           (first (funcall revision))
           (second (funcall revision)))
      (should (natnump first))
      (should (= second first))
      (insert "beta")
      (should (/= (funcall revision) first)))))

(ert-deftest vela-agent-context-snapshot-identifies-the-exact-live-buffer ()
  (let ((first-buffer (generate-new-buffer " *vela-agent-identity*"))
        (second-buffer (generate-new-buffer " *vela-agent-identity*")))
    (unwind-protect
        (let* ((request '(("operation" . "context.snapshot")
                          ("include" . ["buffer"])))
               (identity
                (lambda (buffer)
                  (with-current-buffer buffer
                    (alist-get
                     "identity"
                     (alist-get
                      "buffer"
                      (alist-get
                       "result" (vela-agent-handle-request request)
                       nil nil #'string=)
                      nil nil #'string=)
                     nil nil #'string=))))
               (first-locals
                (with-current-buffer first-buffer (buffer-local-variables)))
               (first (funcall identity first-buffer))
               (repeated (funcall identity first-buffer))
               (second (funcall identity second-buffer)))
          (should (stringp first))
          (with-current-buffer first-buffer
            (should (equal (buffer-local-variables) first-locals)))
          (should (equal repeated first))
          (should-not (equal second first))
          (with-current-buffer first-buffer
            (insert "changed")
            (setq first-locals (buffer-local-variables))
            (should (equal (funcall identity first-buffer) first))
            (should (equal (buffer-local-variables) first-locals)))
          (let ((reused-name (buffer-name first-buffer)))
            (kill-buffer first-buffer)
            (setq first-buffer (generate-new-buffer reused-name))
            (should-not (equal (funcall identity first-buffer) first))))
      (when (buffer-live-p first-buffer)
        (kill-buffer first-buffer))
      (when (buffer-live-p second-buffer)
        (kill-buffer second-buffer)))))

(ert-deftest vela-agent-context-snapshot-identity-survives-feature-reload ()
  (let ((first-buffer (generate-new-buffer " *vela-agent-before-reload*"))
        (second-buffer nil)
        first)
    (unwind-protect
        (progn
          (with-current-buffer first-buffer
            (setq first
                  (alist-get
                   "identity" (vela-agent--buffer-context)
                   nil nil #'string=)))
          (unload-feature 'vela-agent-mode t)
          (require 'vela-agent-mode)
          (with-current-buffer first-buffer
            (should
             (equal
              (alist-get "identity" (vela-agent--buffer-context)
                         nil nil #'string=)
              first)))
          (setq second-buffer
                (generate-new-buffer " *vela-agent-after-reload*"))
          (with-current-buffer second-buffer
            (should-not
             (equal
              (alist-get "identity" (vela-agent--buffer-context)
                         nil nil #'string=)
              first))))
      (when (buffer-live-p first-buffer)
        (kill-buffer first-buffer))
      (when (buffer-live-p second-buffer)
        (kill-buffer second-buffer)))))

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
           '(("protocol_version" . 5)
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
            "{\"protocol_version\":5,\"ok\":true,\"result\":{\"missing\":null,\"enabled\":false,\"items\":[\"a\",\"b\"]}}"))
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
                      (vector t t t t t t t))))
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
        ("include" . ["buffer" "org" "project" "diagnostics" "buffer"])))
     :type 'vela-agent-protocol-error)))

(ert-deftest vela-agent-context-snapshot-rejects-duplicate-sections ()
  (should-error
   (vela-agent-handle-request
    '(("operation" . "context.snapshot")
      ("include" . ["buffer" "org" "buffer"])))
   :type 'vela-agent-protocol-error))

(ert-deftest vela-agent-context-snapshot-validates-section-before-hashing ()
  (cl-letf (((symbol-function 'vela-agent--record-unique-section)
             (lambda (&rest _)
               (ert-fail "oversized section was hashed before validation"))))
    (should-error
     (vela-agent-handle-request
      `(("operation" . "context.snapshot")
        ("include" . [,(make-string
                         (1+ vela-agent-max-operation-characters) ?x)])))
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

(ert-deftest vela-agent-context-snapshot-rejects-oversized-org-metadata ()
  (with-temp-buffer
    (org-mode)
    (insert "* "
            (make-string (1+ vela-agent-max-metadata-string-characters) ?x)
            "\n")
    (goto-char (point-max))
    (should-error
     (vela-agent-handle-request
      '(("operation" . "context.snapshot")
        ("include" . ["org"])))
     :type 'vela-agent-protocol-error)))

;;; vela-agent-mode-test.el ends here
