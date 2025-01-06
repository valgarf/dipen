#!/usr/bin/env bash
rm *.svg && dot -O -Tsvg *.dot && for f in *.svg; do inkscape --actions="page-fit-to-selection;page-fit-to-selection;" --export-margin 2 -lo "${f%.dot.svg}.svg" "$f"; rm "$f"; done