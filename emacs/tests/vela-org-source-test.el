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

(defun vela-org-test--missing-file-links (file)
  "Return missing repository-local file links found in Org FILE."
  (with-temp-buffer
    (insert-file-contents file)
    (setq-local buffer-file-name (expand-file-name file))
    (org-mode)
    (let ((tree (org-element-parse-buffer))
          missing)
      (org-element-map tree 'link
        (lambda (link)
          (when (string= (org-element-property :type link) "file")
            (let* ((path (org-link-unescape
                          (org-element-property :path link)))
                   (target (expand-file-name
                            path (file-name-directory buffer-file-name))))
              (unless (file-exists-p target)
                (push (format "%s:%d -> %s"
                              file
                              (line-number-at-pos
                               (org-element-property :begin link))
                              path)
                      missing))))))
      (nreverse missing))))

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
