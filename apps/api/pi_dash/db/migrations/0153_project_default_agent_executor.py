from django.db import migrations, models

import pi_dash.core.agent_execution


def backfill_local_executor(apps, schema_editor):
    Project = apps.get_model("db", "Project")
    Project.objects.filter(default_agent_executor__isnull=True).update(default_agent_executor="local_runner")


class Migration(migrations.Migration):
    dependencies = [("db", "0152_git_generalization")]

    operations = [
        migrations.AddField(
            model_name="project",
            name="default_agent_executor",
            field=models.CharField(
                blank=True,
                choices=[("local_runner", "Local Runner"), ("cloud_agent", "Pi Dash Cloud Agent")],
                max_length=24,
                null=True,
            ),
        ),
        migrations.RunPython(backfill_local_executor, migrations.RunPython.noop),
        migrations.AlterField(
            model_name="project",
            name="default_agent_executor",
            field=models.CharField(
                choices=[("local_runner", "Local Runner"), ("cloud_agent", "Pi Dash Cloud Agent")],
                default=pi_dash.core.agent_execution.get_default_agent_executor,
                max_length=24,
            ),
        ),
    ]
