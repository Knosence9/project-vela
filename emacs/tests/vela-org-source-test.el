;;; vela-org-source-test.el --- Canonical Org source policy tests -*- lexical-binding: t; -*-

;;; Commentary:

;; Verify that Project Vela keeps one Org-native documentation source tree and
;; that repository-local file links survive document moves.

;;; Code:

(require 'ert)
(require 'org)
(require 'org-element)
(require 'seq)
(require 'subr-x)

(defun vela-org-test--tracked-files (&optional pattern)
  "Return tracked repository files, optionally matching git PATTERN."
  (with-temp-buffer
    (let ((status (if pattern
                      (process-file "git" nil t nil
                                    "ls-files" "-z" "--" pattern)
                    (process-file "git" nil t nil "ls-files" "-z"))))
      (unless (zerop status)
        (error "git ls-files failed%s"
               (if pattern (format " for %s" pattern) "")))
      (split-string (buffer-string) "\0" t))))

(defun vela-org-test--repository-root ()
  "Return the absolute root of the current Git repository."
  (with-temp-buffer
    (unless (zerop (process-file "git" nil t nil
                                 "rev-parse" "--show-toplevel"))
      (error "git rev-parse --show-toplevel failed"))
    (file-name-as-directory (string-trim (buffer-string)))))

(defun vela-org-test--tracked-target-p (target repository-root)
  "Return non-nil when TARGET is tracked below REPOSITORY-ROOT."
  (let ((default-directory repository-root)
        (relative (file-relative-name target repository-root)))
    (zerop (process-file "git" nil nil nil
                         "--literal-pathspecs"
                         "ls-files" "--error-unmatch" "--" relative))))

(defun vela-org-test--custom-id-exists-p (target custom-id)
  "Return non-nil when TARGET contains CUSTOM-ID."
  (with-temp-buffer
    (insert-file-contents target)
    (setq-local buffer-file-name target)
    (org-mode)
    (org-element-map (org-element-parse-buffer) 'headline
      (lambda (headline)
        (equal (org-element-property :CUSTOM_ID headline) custom-id))
      nil t)))

(defun vela-org-test--search-option-resolves-p (target search-option)
  "Return non-nil when TARGET satisfies SEARCH-OPTION."
  (or (null search-option)
      (and (string-prefix-p "#" search-option)
           (> (length search-option) 1)
           (vela-org-test--custom-id-exists-p
            target (substring search-option 1)))))

(defun vela-org-test--missing-file-links (file)
  "Return missing repository-local file links found in Org FILE."
  (with-temp-buffer
    (insert-file-contents file)
    (setq-local buffer-file-name (expand-file-name file))
    (org-mode)
    (let ((tree (org-element-parse-buffer))
          (repository-root (vela-org-test--repository-root))
          missing)
      (org-element-map tree 'link
        (lambda (link)
          (when (string= (org-element-property :type link) "file")
            (let* ((path (org-link-unescape
                          (org-element-property :path link)))
                   (search-option (org-element-property :search-option link))
                   (target (expand-file-name
                            path (file-name-directory buffer-file-name))))
              (unless (and (not (file-name-absolute-p path))
                           (file-in-directory-p target repository-root)
                           (file-exists-p target)
                           (vela-org-test--tracked-target-p
                            target repository-root)
                           (vela-org-test--search-option-resolves-p
                            target search-option))
                (push (format "%s:%d -> %s"
                              file
                              (line-number-at-pos
                               (org-element-property :begin link))
                              path)
                      missing))))))
      (nreverse missing))))

(defmacro vela-org-test--with-temporary-repository (&rest body)
  "Run BODY in a disposable Git repository."
  (declare (indent 0) (debug t))
  `(let* ((repository (make-temp-file "vela-org-link-test-" t))
          (default-directory (file-name-as-directory repository)))
     (unwind-protect
         (progn
           (should (zerop (process-file "git" nil nil nil "init" "--quiet")))
           ,@body)
       (delete-directory repository t))))

(defun vela-org-test--write-fixture (path contents)
  "Write CONTENTS to fixture PATH below `default-directory'."
  (make-directory (file-name-directory (expand-file-name path)) t)
  (with-temp-file path
    (insert contents)))

(ert-deftest vela-org-file-links-reject-absolute-targets ()
  (vela-org-test--with-temporary-repository
    (let ((outside (make-temp-file "vela-org-link-outside-" nil ".org")))
      (unwind-protect
          (progn
            (vela-org-test--write-fixture
             "source.org" (format "[[file:%s][outside]]\n" outside))
            (should (vela-org-test--missing-file-links "source.org")))
        (delete-file outside)))))

(ert-deftest vela-org-file-links-reject-repository-traversal ()
  (vela-org-test--with-temporary-repository
    (let* ((parent (file-name-directory (directory-file-name default-directory)))
           (outside (make-temp-file
                     (expand-file-name "vela-org-link-outside-" parent)
                     nil ".org")))
      (unwind-protect
          (progn
            (vela-org-test--write-fixture
             "source.org"
             (format "[[file:../%s][outside]]\n"
                     (file-name-nondirectory outside)))
            (should (vela-org-test--missing-file-links "source.org")))
        (delete-file outside)))))

(ert-deftest vela-org-file-links-reject-untracked-targets ()
  (vela-org-test--with-temporary-repository
    (vela-org-test--write-fixture "source.org" "[[file:target.org][target]]\n")
    (vela-org-test--write-fixture "target.org" "* Target\n")
    (should (zerop (process-file "git" nil nil nil "add" "--" "source.org")))
    (should (vela-org-test--missing-file-links "source.org"))))

(ert-deftest vela-org-file-links-treat-target-paths-literally ()
  (vela-org-test--with-temporary-repository
    (vela-org-test--write-fixture
     "source.org" "[[file:target-*.org][literal target]]\n")
    (vela-org-test--write-fixture "target-*.org" "* Untracked literal target\n")
    (vela-org-test--write-fixture "target-tracked.org" "* Tracked glob match\n")
    (should (zerop (process-file
                    "git" nil nil nil "add" "--"
                    "source.org" "target-tracked.org")))
    (should (vela-org-test--missing-file-links "source.org"))))

(ert-deftest vela-org-file-links-reject-missing-custom-id ()
  (vela-org-test--with-temporary-repository
    (vela-org-test--write-fixture
     "source.org" "[[file:target.org::#missing][target]]\n")
    (vela-org-test--write-fixture "target.org" "* Target\n")
    (should (zerop (process-file
                    "git" nil nil nil "add" "--" "source.org" "target.org")))
    (should (vela-org-test--missing-file-links "source.org"))))

(ert-deftest vela-org-file-links-accept-existing-custom-id ()
  (vela-org-test--with-temporary-repository
    (vela-org-test--write-fixture
     "source.org" "[[file:target.org::#present][target]]\n")
    (vela-org-test--write-fixture
     "target.org" "* Target\n:PROPERTIES:\n:CUSTOM_ID: present\n:END:\n")
    (should (zerop (process-file
                    "git" nil nil nil "add" "--" "source.org" "target.org")))
    (should-not (vela-org-test--missing-file-links "source.org"))))

(ert-deftest vela-org-sources-replace-authored-markdown ()
  (let ((markdown-files
         (seq-filter
          (lambda (file)
            (member (downcase (or (file-name-extension file) ""))
                    '("md" "markdown")))
          (vela-org-test--tracked-files)))
        (org-files (vela-org-test--tracked-files "*.org")))
    (should-not markdown-files)
    (should org-files)))

(ert-deftest vela-org-file-links-resolve ()
  (let* ((org-files (vela-org-test--tracked-files "*.org"))
         (missing (apply #'append
                         (mapcar #'vela-org-test--missing-file-links org-files))))
    (should-not missing)))

(provide 'vela-org-source-test)

;;; vela-org-source-test.el ends here
