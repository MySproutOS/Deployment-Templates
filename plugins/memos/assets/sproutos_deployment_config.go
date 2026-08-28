package store

import (
	"context"
	"os"
	"sort"
	"strconv"
	"strings"

	"github.com/pkg/errors"

	storepb "github.com/usememos/memos/proto/gen/store"
)

const sproutOSStorageID = "sproutos-managed"

var sproutOSStorageEnvironment = []string{
	"S3_ACCESS_KEY_ID",
	"S3_BUCKET_NAME",
	"S3_ENDPOINT",
	"S3_FORCE_PATH_STYLE",
	"S3_REGION",
	"S3_SECRET_ACCESS_KEY",
}

// LoadSproutOSDeploymentConfiguration loads Memos' file-backed deployment configuration and then
// overlays the managed SproutOS object store without serializing credentials to disk or logs.
func (s *Store) LoadSproutOSDeploymentConfiguration(ctx context.Context) error {
	if err := s.LoadDeploymentConfiguration(ctx); err != nil {
		return err
	}

	values := make(map[string]string, len(sproutOSStorageEnvironment))
	missing := make([]string, 0, len(sproutOSStorageEnvironment))
	present := 0
	for _, name := range sproutOSStorageEnvironment {
		value, ok := os.LookupEnv(name)
		if ok {
			present++
			values[name] = value
		} else {
			missing = append(missing, name)
		}
	}
	if present == 0 {
		return nil
	}
	if len(missing) != 0 {
		sort.Strings(missing)
		return errors.Errorf("incomplete SproutOS object storage environment; missing %s", strings.Join(missing, ", "))
	}
	usePathStyle, err := strconv.ParseBool(values["S3_FORCE_PATH_STYLE"])
	if err != nil {
		return errors.New("S3_FORCE_PATH_STYLE must be true or false")
	}

	setting := &storepb.InstanceSetting{
		Key: storepb.InstanceSettingKey_STORAGE,
		Value: &storepb.InstanceSetting_StorageSetting{StorageSetting: &storepb.InstanceStorageSetting{
			Storages: []*storepb.Storage{{
				Id:   sproutOSStorageID,
				Name: "SproutOS managed object storage",
				Type: storepb.StorageType_STORAGE_TYPE_S3,
				Config: &storepb.Storage_S3Config{S3Config: &storepb.StorageS3Config{
					AccessKeyId:     values["S3_ACCESS_KEY_ID"],
					AccessKeySecret: values["S3_SECRET_ACCESS_KEY"],
					Endpoint:        values["S3_ENDPOINT"],
					Region:          values["S3_REGION"],
					Bucket:          values["S3_BUCKET_NAME"],
					UsePathStyle:    usePathStyle,
				}},
			}},
			DefaultStorageId: sproutOSStorageID,
		}},
	}
	if err := validateAndNormalizeDeploymentInstanceSetting(setting); err != nil {
		return errors.Wrap(err, "invalid SproutOS object storage configuration")
	}

	config := newDeploymentConfiguration()
	s.deploymentConfigMu.RLock()
	if _, exists := s.deploymentConfig.instanceSettings[storepb.InstanceSettingKey_STORAGE]; exists {
		s.deploymentConfigMu.RUnlock()
		return errors.New("SproutOS managed object storage conflicts with a file-backed STORAGE setting")
	}
	for uid, provider := range s.deploymentConfig.identityProviders {
		config.identityProviders[uid] = cloneIdentityProvider(provider)
	}
	for key, configured := range s.deploymentConfig.instanceSettings {
		config.instanceSettings[key] = cloneInstanceSetting(configured)
	}
	s.deploymentConfigMu.RUnlock()
	config.instanceSettings[storepb.InstanceSettingKey_STORAGE] = cloneInstanceSetting(setting)
	s.setDeploymentConfiguration(config)
	return nil
}
