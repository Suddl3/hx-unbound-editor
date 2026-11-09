# Experimental source preview status

The public preview starts offline at every URL. Device access requires the
explicit `-ExperimentalDevice` launcher switch and a Connect action. The server
rejects IR/capture installation, sample inspection and named NAM selection even
in that mode. Local capture import/classification is available without USB.
This restriction is enforced by the API, not just disabled buttons.

## Established on the development Floor

Named A2 Lite selection, two-way stock bypass/parameter changes, some block
moves and preset/snapshot operations have passed specific physical checks.
These results are scoped to their tested sequences; they are not comprehensive
coverage or a promise of crash-free operation.

## Unresolved

- A persistent "Transferring data" screen after capture verification/selection.
  A Floor-specific IR transfer termination difference in the pinned transport
  is an investigation lead, not a proven fix.
- Other combined-operation panel freezes and broader stress coverage.
- Stock HX Edit backup/restore does not preserve custom model 380 correctly.
- The new Floor model-555 compatibility approach is reported upstream but is
  not integrated or validated here. IDs and parameter layouts must not be
  changed without a migration plan.
- DSP accounting and current upstream Floor cost differ. No measured A2 Full
  capacity or general maximum NAM count is promised by this editor.
- The offline launcher, library import/classification, favorites, list view and
  theme persistence have been exercised against the real local server in a fresh
  extracted folder. The bridge also builds from that folder. This used existing
  development runtimes; installation on a clean Windows machine remains untested.
- The legacy transfer view is excluded. The live bridge builds from included source.
- Stock metadata and optional stock artwork come from an installed copy of HX
  Edit; the package contains no extracted stock art. Local NAM browsing uses the
  app's own illustrations and works without that installation.

## Product split

This repository owns the editor/librarian. Firmware contributions should be
small, independently reviewable changes against maintained firmware sources.
Do not publish a second engine baseline merely to duplicate the existing Floor
port. The [upstream reconciliation](UPSTREAM.md) now identifies the overlapping
interrupt/PM fixes, a tested offline DMA candidate and additional tests for an
existing transport PR. None of those prepared contributions has been submitted
or adopted into this package's runtime dependencies.

This package is an extracted source candidate. It does not replace the working
development installation, synchronize any fork or create a GitHub repository.
