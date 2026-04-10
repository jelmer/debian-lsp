# Emacs

## Prerequisites

- Emacs 29+ (ships with `eglot` built-in)
- The `debian-lsp` binary available on your system

## Installation

### 1. Make sure `debian-lsp` is on your PATH

```bash
which debian-lsp
```

If the command returns nothing, copy the binary to a directory on your PATH:

```bash
sudo cp /path/to/debian-lsp /usr/local/bin/
sudo chmod +x /usr/local/bin/debian-lsp
```

### 2. Configure your `init.el`

Your `init.el` should be located at `~/.emacs.d/init.el`. Create it if it doesn't exist:

```bash
mkdir -p ~/.emacs.d
touch ~/.emacs.d/init.el
```

Add the following configuration:

```elisp
;; MELPA – package repository
(require 'package)
(add-to-list 'package-archives
             '("melpa" . "https://melpa.org/packages/") t)
(package-initialize)

;; eglot is built-in since Emacs 29
(require 'eglot)

;; company – completion frontend
;; eglot speaks LSP but does not display completion suggestions on its own.
;; A frontend like company (or corfu) is required for completions to appear.
(unless (package-installed-p 'company)
  (package-refresh-contents)
  (package-install 'company))
(require 'company)
(setq company-idle-delay 0.2)
(setq company-minimum-prefix-length 1)

;; Define Debian-specific major modes
;; Emacs does not recognise these file types natively.
(define-derived-mode debcontrol-mode       fundamental-mode "debcontrol")
(define-derived-mode debcopyright-mode     fundamental-mode "debcopyright")
(define-derived-mode debchangelog-mode     fundamental-mode "debchangelog")
(define-derived-mode debwatch-mode         fundamental-mode "debwatch")
(define-derived-mode debrules-mode         fundamental-mode "debrules")
(define-derived-mode debsources-mode       fundamental-mode "debsources")
(define-derived-mode debsourceoptions-mode fundamental-mode "debsourceoptions")
(define-derived-mode debupstream-mode      fundamental-mode "debupstream")
(define-derived-mode debpatches-mode       fundamental-mode "debpatches")
(define-derived-mode autopkgtest-mode      fundamental-mode "autopkgtest")

;; Associate Debian packaging files with their modes
(add-to-list 'auto-mode-alist '("debian/control\\'"               . debcontrol-mode))
(add-to-list 'auto-mode-alist '("debian/copyright\\'"            . debcopyright-mode))
(add-to-list 'auto-mode-alist '("debian/changelog\\'"            . debchangelog-mode))
(add-to-list 'auto-mode-alist '("debian/changelog\\.dch\\'"      . debchangelog-mode))
(add-to-list 'auto-mode-alist '("debian/watch\\'"                . debwatch-mode))
(add-to-list 'auto-mode-alist '("debian/rules\\'"                . debrules-mode))
(add-to-list 'auto-mode-alist '("debian/source/format\\'"        . debsources-mode))
(add-to-list 'auto-mode-alist '("debian/source/options\\'"       . debsourceoptions-mode))
(add-to-list 'auto-mode-alist '("debian/source/local-options\\'" . debsourceoptions-mode))
(add-to-list 'auto-mode-alist '("debian/upstream/metadata\\'"    . debupstream-mode))
(add-to-list 'auto-mode-alist '("debian/patches/series\\'"      . debpatches-mode))
(add-to-list 'auto-mode-alist '("debian/tests/control\\'"        . autopkgtest-mode))

;; Register debian-lsp and enable company for all Debian modes
(dolist (mode '(debcontrol-mode
                debcopyright-mode
                debchangelog-mode
                debwatch-mode
                debrules-mode
                debsources-mode
                debsourceoptions-mode
                debupstream-mode
                debpatches-mode
                autopkgtest-mode))
  (add-to-list 'eglot-server-programs
               `(,mode . ("debian-lsp")))
  (add-hook (intern (concat (symbol-name mode) "-hook")) #'eglot-ensure)
  (add-hook (intern (concat (symbol-name mode) "-hook")) #'company-mode))
```
