import re

with open('app/crates/backend-repository/src/traits.rs', 'r') as f:
    content = f.read()

# Remove SmInstanceFilter, SmInstanceCreateInput, SmEventCreateInput, SmStepAttemptCreateInput, SmStepAttemptPatch
content = re.sub(r'/// Filter criteria for state machine instance queries\..*?}\n\n', '', content, flags=re.DOTALL)
content = re.sub(r'impl SmInstanceFilter.*?}\n\n', '', content, flags=re.DOTALL)
content = re.sub(r'/// Input for creating a new state machine instance\..*?}\n\n', '', content, flags=re.DOTALL)
content = re.sub(r'/// Input for creating a state machine event\..*?}\n\n', '', content, flags=re.DOTALL)
content = re.sub(r'/// Input for creating a step attempt record\..*?}\n\n', '', content, flags=re.DOTALL)
content = re.sub(r'/// Partial update for step attempt records\..*?}\n\n', '', content, flags=re.DOTALL)
content = re.sub(r'impl SmStepAttemptPatch.*?}\n\n', '', content, flags=re.DOTALL)
content = re.sub(r'/// Output of finding an active step attempt.*?\n\n', '', content, flags=re.DOTALL)

# Remove StateMachineRepo trait but KEEP sync_deposit_recipients and select_deposit_recipient_contact
# Actually, it's easier to just do it via regex
sm_trait_start = content.find('/// Repository trait for generic state machine operations.')
user_trait_start = content.find('/// Repository trait for user account operations.')

sm_trait_content = content[sm_trait_start:user_trait_start]

# We want to extract sync_deposit_recipients and select_deposit_recipient_contact
sync_match = re.search(r'(/// Replaces configured deposit recipients.*?)(?=\n})', sm_trait_content, re.DOTALL)
if sync_match:
    sync_methods = sync_match.group(1)
    # Insert them into FlowRepo trait
    flow_trait_end = content.find('}', content.find('pub trait FlowRepo: Send + Sync {'))
    if flow_trait_end != -1:
        content = content[:flow_trait_end] + "    " + sync_methods + "\n" + content[flow_trait_end:]

content = content[:sm_trait_start] + content[user_trait_start:]

with open('app/crates/backend-repository/src/traits.rs', 'w') as f:
    f.write(content)
