"""Build one-ingredient hybrids between a Negs profile and its Fotufilm conversion."""
import json, sys
from pathlib import Path
negs_id, conv_id, data_dir = sys.argv[1], sys.argv[2], Path(sys.argv[3])
N = json.load(open(data_dir / "profiles" / f"{negs_id}.json"))
F = json.load(open(data_dir / "profiles" / f"{conv_id}.json"))
groups = {
    "window": ["hanatos2025_adaptation_window_params"],
    "dyes": ["channel_density"],
    "base": ["base_density"],
    "curves": ["log_exposure", "density_curves", "density_curves_layers", "density_curves_model"],
    "sens": ["log_sensitivity"],
}
for g, keys in groups.items():
    P = json.loads(json.dumps(N))
    for k in keys:
        P["data"][k] = F["data"].get(k)
    P["info"]["stock"] = f"hyb_{negs_id}__{g}"
    (data_dir / "profiles" / f"hyb_{negs_id}__{g}.json").write_text(json.dumps(P))
    print(P["info"]["stock"])
