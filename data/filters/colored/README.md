# Camera colour filters

`hoya/*.csv` are copied unchanged from Andrea Volpato's
[spektrafilm](https://github.com/andreavolpato/spektrafilm) at commit
`28bf883e1672e884307edc75852549376e13644e` (`src/spektrafilm/data/filters/colored/hoya/`,
added in `820de14`). They are spectral transmittance curves of Hoya colour
glass filters (X0, X1, Y2, YA3, R1), as (wavelength in nm, transmittance)
rows. Upstream gives no separate source or licence note for them; they are
part of spektrafilm's GPL-3.0 code tree (its CC BY-SA 4.0 licence covers the
profiles and LUTs only), and Negs is GPL-3.0 as well.

`spektrafilm-core::color_filters` reads them as upstream does: duplicate
wavelengths keep their first row, and an Akima interpolator resamples them
onto the 380–780 nm / 5 nm grid.
