# Negs (unreleased)

## All platforms

- **Preflash on the GPU.** Prints with a preflash now come out the same on
  the GPU as in the reference render when the print exposure isn't 1 or the
  scanner's white correction is on. Before, such prints differed from the
  reference by up to about 14 ΔE00 at a print exposure of 0.7.
- More accurate wide blurs on the CPU fallback (32-bit), most visibly with
  wide DIR couplers.
- Moving the enlarger's M/Y shifts or the preflash no longer rebuilds the
  engine's calibration for each preview.

## Web

- Negs runs in the browser at [getnegs.com/try](https://getnegs.com/try/).
