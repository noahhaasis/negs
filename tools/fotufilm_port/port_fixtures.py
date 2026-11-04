"""Convert Fotufilm stocks that Negs already ships, under fotufilm_* ids, as test-only references."""
import sys
from pathlib import Path
import port_stocks as ps

FIXTURES = {"gold200": ("fotufilm_gold200", "Kodak Gold 200 (Fotufilm reference)", "kodak_portra_endura", False),
            "provia100f": ("fotufilm_provia100f", "Fujifilm Provia 100F (Fotufilm reference)", None, False),
            "superia400": ("fotufilm_superia400", "Fujifilm Superia X-TRA 400 (Fotufilm reference)", "fujifilm_crystal_archive_typeii", False),
            "ektachromee100": ("fotufilm_ektachromee100", "Kodak Ektachrome E100 (Fotufilm reference)", None, False)}
ps.MANIFEST.update(FIXTURES)
ps.POS.update({k: v for k, v in FIXTURES.items() if v[2] is None})
if __name__ == "__main__":
    data_dir = Path(sys.argv[1]).resolve()
    for fid in sys.argv[2:] or FIXTURES:
        ps.port(fid, data_dir)
