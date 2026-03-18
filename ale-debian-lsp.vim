" ALE configuration for debian-lsp
" Source this file in your .vimrc or init.vim:
"   source /path/to/debian-lsp/ale-debian-lsp.vim
"
" You can customize the executable path by setting g:debian_lsp_executable
" before sourcing this file:
"   let g:debian_lsp_executable = '/custom/path/to/debian-lsp'
"   source /path/to/debian-lsp/ale-debian-lsp.vim

" Set default executable path if not already configured
if !exists('g:debian_lsp_executable')
  let g:debian_lsp_executable = expand('<sfile>:p:h') . '/target/release/debian-lsp'
endif

" Register debian-lsp with ALE for all supported file types
let g:ale_linters = get(g:, 'ale_linters', {})
let g:ale_linters.debcontrol = ['debian-lsp']
let g:ale_linters.debcopyright = ['debian-lsp']
let g:ale_linters.debchangelog = ['debian-lsp']
let g:ale_linters.debsources = ['debian-lsp']
let g:ale_linters.debwatch = ['debian-lsp']
let g:ale_linters.debupstream = ['debian-lsp']
let g:ale_linters.autopkgtest = ['debian-lsp']
let g:ale_linters.debrules = ['debian-lsp']

" Define debian-lsp for debian/control files
call ale#linter#Define('debcontrol', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': g:debian_lsp_executable,
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})

" Define debian-lsp for debian/copyright files
call ale#linter#Define('debcopyright', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': g:debian_lsp_executable,
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})

" Define debian-lsp for debian/changelog files
call ale#linter#Define('debchangelog', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': g:debian_lsp_executable,
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})

" Define debian-lsp for debian/source/format files
call ale#linter#Define('debsources', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': g:debian_lsp_executable,
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})

" Define debian-lsp for debian/watch files
call ale#linter#Define('debwatch', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': g:debian_lsp_executable,
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})

" Define debian-lsp for debian/upstream/metadata files
call ale#linter#Define('debupstream', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': g:debian_lsp_executable,
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})

" Define debian-lsp for debian/rules files
call ale#linter#Define('debrules', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': g:debian_lsp_executable,
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})

" Define debian-lsp for debian/tests/control files
call ale#linter#Define('autopkgtest', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': g:debian_lsp_executable,
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})

" Set filetypes for Debian packaging files
" Note: Vim already detects debcontrol, debcopyright, debchangelog,
" debsources, and autopkgtest.
augroup debian_filetypes
  autocmd!
  autocmd BufNewFile,BufRead */debian/control setfiletype debcontrol
  autocmd BufNewFile,BufRead */debian/copyright setfiletype debcopyright
  autocmd BufNewFile,BufRead */debian/changelog setfiletype debchangelog
  autocmd BufNewFile,BufRead */debian/source/format setfiletype debsources
  autocmd BufNewFile,BufRead */debian/watch setfiletype debwatch
  autocmd BufNewFile,BufRead */debian/upstream/metadata setfiletype debupstream
  autocmd BufNewFile,BufRead */debian/rules setfiletype debrules
augroup END
