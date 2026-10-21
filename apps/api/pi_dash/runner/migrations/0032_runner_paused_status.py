from django.db import migrations, models


def forwards(apps, schema_editor):
    # 0031 recorded a pause only in ``paused_at``; the status is now the
    # source of truth.
    Runner = apps.get_model("runner", "Runner")
    Runner.objects.filter(paused_at__isnull=False, revoked_at__isnull=True).exclude(status="revoked").update(
        status="paused"
    )


def backwards(apps, schema_editor):
    # ``paused_at`` is kept, so 0031's logic still sees the pause; the next
    # poll restores the real connectivity status.
    Runner = apps.get_model("runner", "Runner")
    Runner.objects.filter(status="paused").update(status="offline")


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0031_runner_paused"),
    ]

    operations = [
        migrations.AlterField(
            model_name="runner",
            name="status",
            field=models.CharField(
                choices=[
                    ("online", "Online"),
                    ("offline", "Offline"),
                    ("busy", "Busy"),
                    ("revoked", "Revoked"),
                    ("paused", "Paused"),
                ],
                db_index=True,
                default="offline",
                max_length=16,
            ),
        ),
        migrations.RunPython(forwards, backwards),
    ]
