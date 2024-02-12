import * as zod from 'zod'
import { StepProps } from '../types';
import { Field } from 'formik';
import { ErrorDisplay } from '../../../components/Error';
import { Alert } from '../../../components/ui/alert';
import { useForm } from '@tanstack/react-form';
import { zodValidator } from '@tanstack/zod-form-adapter';


export const adminUserPluginSchema = zod.object({
    type: zod.literal("hallo"),
    hello: zod.string(),
    usernameField: zod.optional(zod.string()).default("adminUsername"),
    passwordField:  zod.optional(zod.string()).default("adminUsername"),
})


type Values = zod.infer<typeof adminUserPluginSchema>



export const AdminUserForm: React.FC<StepProps<Values>> = ({ errors, schema, onSubmit}) => {
    const form = useForm({
        defaultValues: {
          [schema.usernameField]: "admin",
          [schema.passwordField]: "admin",
        },
        onSubmit: async ({ value }) => {
          // Do something with form data
          console.log(value)
        },

        validatorAdapter: zodValidator
      })



    return (
      <div className="h-full w-full my-7 flex flex-col">
        <div className="font-light text-7xl"> Attention ! 🦸‍♂️</div>
        <div className="font-light text-2xl mt-4">
          Your Arkitekt instance needs an Admin 
        </div>
        <div className="mb-2 text-justify mt-4 max-w-xl">
          Administrators are power users that can manage the Arkitekt instance.
          and have access to all data. You can add more administrators later. This
          admin user will be created during the setup process and will be able to
          access every services that is running on this instance.
        </div>
        <div className="max-w-xl">
        
            <form.Field
              name={schema.usernameField}
              validators={{
                onChange: zod.string(),
                onChangeAsyncDebounceMs: 500,
              }}
              children={(field) => {
                // Avoid hasty abstractions. Render props are great!
                return (
                  <>
                    <label htmlFor={field.name}>Admin Username</label>
                    <input
                      id={field.name}
                      name={field.name}
                      className="text-center border border-gray-400 rounded p-2 text-black"
                      value={field.state.value}
                      onBlur={field.handleBlur}
                      onChange={(e) => field.handleChange(e.target.value)}
                    />
                  </>
                )
              }}
            />
          </div>
          <div>
            <form.Field
               name={schema.passwordField}
               validators={{
                 onChange: zod.string(),
                 onChangeAsyncDebounceMs: 500,
               }}
              children={(field) => (
                <>
                  <label htmlFor={field.name}>Password</label>
                  <input
                    id={field.name}
                    name={field.name}
                    type="password"
                    value={field.state.value}
                    onBlur={field.handleBlur}
                    onChange={(e) => field.handleChange(e.target.value)}
                  />
                </>
              )}
            />
          </div>
          <form.Subscribe
            selector={(state) => [state.canSubmit, state.isSubmitting]}
            children={([canSubmit, isSubmitting]) => (
              <button type="submit" disabled={!canSubmit}>
                {isSubmitting ? '...' : 'Submit'}
              </button>
            )}
          />
          <Alert variant="destructive" className="mt-4 ">
            Important, please never sign in as a admin user when using the platform as a normal user. (e.g. to create workflows).
            You should only use the admin user to manage the platform.
          </Alert>

      </div>
    );
  };


export const AdminUserForm