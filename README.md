TODO:

Support this syntax:

```ini
[db]
cmd = make pgup
ready_check = database is ready

[api]
cmd = make api
depends_on = db
ready_check = ready and listening

[web]
cmd = make front
depends_on = api
ready_check = ready in
```
