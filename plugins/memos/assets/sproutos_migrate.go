// Command sproutos-migrate is the single-purpose Memos schema migrator used by SproutOS.
// It is built as an AWS Lambda provided.al2023 custom runtime so migrations run before the
// request-serving version receives traffic. Running the binary outside Lambda performs one
// migration directly, which keeps the exact artifact locally testable.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"

	"golang.org/x/crypto/bcrypt"
	"google.golang.org/protobuf/proto"

	"github.com/usememos/memos/internal/profile"
	"github.com/usememos/memos/internal/version"
	storepb "github.com/usememos/memos/proto/gen/store"
	"github.com/usememos/memos/store"
	"github.com/usememos/memos/store/db"
)

const runtimeAPIVersion = "2018-06-01"

const (
	adminUsername         = "admin"
	adminBootstrapMarker  = "sproutos:pending-admin-bootstrap"
	minimumPasswordLength = 32
	maximumPasswordLength = 72
)

func main() {
	runtimeAPI := strings.TrimSpace(os.Getenv("AWS_LAMBDA_RUNTIME_API"))
	if runtimeAPI == "" {
		if err := migrate(context.Background()); err != nil {
			fmt.Fprintf(os.Stderr, "Memos controlled migration failed: %v\n", err)
			os.Exit(1)
		}
		fmt.Println("Memos controlled migration completed")
		return
	}

	requestID, err := nextInvocation(runtimeAPI)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Memos migration runtime failed to receive an invocation: %v\n", err)
		os.Exit(1)
	}
	if err := migrate(context.Background()); err != nil {
		fmt.Fprintf(os.Stderr, "Memos controlled migration failed: %v\n", err)
		if reportErr := reportError(runtimeAPI, requestID, err); reportErr != nil {
			fmt.Fprintf(os.Stderr, "Memos migration runtime failed to report the error: %v\n", reportErr)
		}
		os.Exit(1)
	}
	if err := reportSuccess(runtimeAPI, requestID); err != nil {
		fmt.Fprintf(os.Stderr, "Memos migration runtime failed to report success: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("Memos controlled migration completed")
}

func migrate(ctx context.Context) error {
	dsn := strings.TrimSpace(os.Getenv("MEMOS_DSN"))
	if dsn == "" {
		return fmt.Errorf("MEMOS_DSN is required")
	}
	adminPassword, err := validatedAdminPassword()
	if err != nil {
		return err
	}
	instanceProfile := &profile.Profile{
		Driver:  "postgres",
		DSN:     dsn,
		Data:    "/tmp/memos-migration",
		Version: version.GetCurrentVersion(),
		Commit:  version.Commit,
	}
	if err := instanceProfile.Validate(); err != nil {
		return fmt.Errorf("validate migration profile: %w", err)
	}
	driver, err := db.NewDBDriver(instanceProfile)
	if err != nil {
		return fmt.Errorf("create database driver: %w", err)
	}
	storeInstance := store.New(driver, instanceProfile)
	defer storeInstance.Close()
	if err := storeInstance.Migrate(ctx); err != nil {
		return fmt.Errorf("migrate database: %w", err)
	}
	if err := bootstrapOwnership(ctx, storeInstance, adminPassword); err != nil {
		return fmt.Errorf("bootstrap administrator: %w", err)
	}
	return nil
}

func validatedAdminPassword() (string, error) {
	password, ok := os.LookupEnv("MEMOS_ADMIN_PASSWORD")
	if !ok || password == "" {
		return "", fmt.Errorf("MEMOS_ADMIN_PASSWORD is required")
	}
	if password != strings.TrimSpace(password) {
		return "", fmt.Errorf("MEMOS_ADMIN_PASSWORD must not start or end with whitespace")
	}
	if len(password) < minimumPasswordLength || len(password) > maximumPasswordLength {
		return "", fmt.Errorf(
			"MEMOS_ADMIN_PASSWORD must contain between %d and %d bytes",
			minimumPasswordLength,
			maximumPasswordLength,
		)
	}
	lower := strings.ToLower(password)
	knownDefaults := map[string]struct{}{
		"admin": {}, "changeme": {}, "change-me": {}, "memos": {}, "password": {},
		"memos-admin-password": {}, "passwordpasswordpasswordpassword": {}, "replace-me": {},
	}
	if _, found := knownDefaults[lower]; found || allBytesEqual(password) {
		return "", fmt.Errorf("MEMOS_ADMIN_PASSWORD must not be a default or repeated value")
	}
	return password, nil
}

func allBytesEqual(value string) bool {
	for index := 1; index < len(value); index++ {
		if value[index] != value[0] {
			return false
		}
	}
	return true
}

// bootstrapOwnership closes upstream Memos' unauthenticated first-user race before traffic. A
// marker on the new user makes the two store mutations restart-safe: a failed migration retries
// registration lockdown, while a completed bootstrap never overwrites an administrator's later
// password or registration choice.
func bootstrapOwnership(ctx context.Context, storeInstance *store.Store, password string) error {
	seededAdmin, err := storeInstance.GetUser(ctx, &store.FindUser{Username: pointer(adminUsername)})
	if err != nil {
		return fmt.Errorf("find bootstrap administrator: %w", err)
	}
	if seededAdmin != nil {
		if seededAdmin.Role != store.RoleAdmin {
			return fmt.Errorf("reserved username %q exists without the ADMIN role", adminUsername)
		}
		if seededAdmin.Nickname == adminBootstrapMarker {
			return finishOwnershipBootstrap(ctx, storeInstance, seededAdmin)
		}
		return nil
	}

	limitOne := 1
	existingUsers, err := storeInstance.ListUsers(ctx, &store.FindUser{Limit: &limitOne})
	if err != nil {
		return fmt.Errorf("list existing users: %w", err)
	}
	if len(existingUsers) != 0 {
		adminRole := store.RoleAdmin
		existingAdmins, err := storeInstance.ListUsers(ctx, &store.FindUser{Role: &adminRole, Limit: &limitOne})
		if err != nil {
			return fmt.Errorf("list existing administrators: %w", err)
		}
		if len(existingAdmins) == 0 {
			return fmt.Errorf("database contains users but no administrator")
		}
		return nil
	}

	passwordHash, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		return fmt.Errorf("hash administrator password: %w", err)
	}
	seededAdmin, created, err := storeInstance.CreateUserIfNoUsers(ctx, &store.User{
		Username:     adminUsername,
		Role:         store.RoleAdmin,
		Nickname:     adminBootstrapMarker,
		PasswordHash: string(passwordHash),
	})
	if err != nil {
		return fmt.Errorf("create administrator: %w", err)
	}
	if !created {
		return fmt.Errorf("another user appeared during administrator bootstrap")
	}
	return finishOwnershipBootstrap(ctx, storeInstance, seededAdmin)
}

func finishOwnershipBootstrap(ctx context.Context, storeInstance *store.Store, seededAdmin *store.User) error {
	general, err := storeInstance.GetInstanceGeneralSetting(ctx)
	if err != nil {
		return fmt.Errorf("read registration setting: %w", err)
	}
	if general == nil {
		general = &storepb.InstanceGeneralSetting{}
	} else {
		general = proto.Clone(general).(*storepb.InstanceGeneralSetting)
	}
	general.DisallowUserRegistration = true
	if _, err := storeInstance.UpsertInstanceGeneralSettingSafely(ctx, &storepb.InstanceSetting{
		Key:   storepb.InstanceSettingKey_GENERAL,
		Value: &storepb.InstanceSetting_GeneralSetting{GeneralSetting: general},
	}); err != nil {
		return fmt.Errorf("disable public registration: %w", err)
	}

	nickname := "Administrator"
	if _, err := storeInstance.UpdateUser(ctx, &store.UpdateUser{
		ID:       seededAdmin.ID,
		Nickname: &nickname,
	}); err != nil {
		return fmt.Errorf("complete administrator bootstrap: %w", err)
	}
	return nil
}

func pointer[T any](value T) *T {
	return &value
}

func nextInvocation(runtimeAPI string) (string, error) {
	response, err := http.Get(runtimeURL(runtimeAPI, "runtime/invocation/next")) // #nosec G107 -- AWS supplies the runtime endpoint.
	if err != nil {
		return "", err
	}
	defer response.Body.Close()
	if _, err := io.Copy(io.Discard, response.Body); err != nil {
		return "", err
	}
	if response.StatusCode != http.StatusOK {
		return "", fmt.Errorf("runtime next returned %s", response.Status)
	}
	requestID := response.Header.Get("Lambda-Runtime-Aws-Request-Id")
	if requestID == "" {
		return "", fmt.Errorf("runtime next omitted Lambda-Runtime-Aws-Request-Id")
	}
	return requestID, nil
}

func reportSuccess(runtimeAPI, requestID string) error {
	return postRuntime(runtimeURL(runtimeAPI, "runtime/invocation/"+requestID+"/response"), []byte(`{"ok":true}`))
}

func reportError(runtimeAPI, requestID string, migrationError error) error {
	payload, err := json.Marshal(map[string]string{
		"errorMessage": migrationError.Error(),
		"errorType":    "MemosControlledMigrationError",
	})
	if err != nil {
		return err
	}
	return postRuntime(runtimeURL(runtimeAPI, "runtime/invocation/"+requestID+"/error"), payload)
}

func postRuntime(url string, payload []byte) error {
	response, err := http.Post(url, "application/json", bytes.NewReader(payload)) // #nosec G107 -- AWS supplies the runtime endpoint.
	if err != nil {
		return err
	}
	defer response.Body.Close()
	if _, err := io.Copy(io.Discard, response.Body); err != nil {
		return err
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return fmt.Errorf("runtime response returned %s", response.Status)
	}
	return nil
}

func runtimeURL(runtimeAPI, path string) string {
	return "http://" + runtimeAPI + "/" + runtimeAPIVersion + "/" + path
}
